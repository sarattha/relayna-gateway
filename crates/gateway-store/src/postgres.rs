use async_trait::async_trait;
use chrono::Datelike;
use gateway_core::{
    admin::{
        AdminKeyCreate, AdminKeyOwnerType, AdminKeyPatch, AdminKeyResponse,
        AdminPolicyLayerResponse, AdminPolicyLayerUpsert, AdminPolicyResponse,
    },
    auth::{StoredVirtualKey, VirtualKeyLookup},
    default_route_pattern, operator_token_prefix, parse_provider_config_kind,
    projects::{ProjectCreateRequest, ProjectPatchRequest, ProjectResponse},
    provider_config_kind_str,
    provider_configs::{
        AdminProviderConfigStore, ProviderConfigCreateRequest, ProviderConfigLookup,
        ProviderConfigPatchRequest, ProviderConfigResponse, ProviderRuntimeConfig,
    },
    resolve_effective_policy,
    services::{
        AdminServiceStore, ServiceCostMode, ServiceCreateRequest, ServicePatchRequest,
        ServiceRegistration, ServiceRegistryLookup, ServiceResponse, ServiceRouteLookup,
        ServiceSource, ServiceSyncStatus, ServiceSyncStatusResponse, StudioServiceImportRequest,
    },
    studio_settings::{
        normalize_base_url, normalize_secret, AdminStudioConnectionStore, PatchValue,
        StoredStudioConnection, StudioConnectionPatchRequest,
    },
    verify_stored_operator_token, AdminAuditStore, AdminGuardrailDefinitionResponse, AdminKeyStore,
    AdminKeyUsageSummary, AdminOpenAiRouteStore, AdminPolicyLayerStore, AdminProjectStore,
    AuditEvent, AuditEventCreate, AuditEventQuery, CircuitBreakerState, DebugBundle,
    FallbackAttempt, GatewayError, GatewayResult, GuardrailAdminCreateRequest,
    GuardrailAdminPatchRequest, GuardrailDefinition, GuardrailEventQuery, GuardrailExecutionEvent,
    GuardrailExecutionSummary, GuardrailMode, GuardrailObservabilityStore, GuardrailPolicy,
    GuardrailProviderKind, GuardrailStore, KeyPolicy, OpenAiRouteSetting,
    OpenAiRouteSettingsLookup, OperatorAuthorization, OperatorTokenMaterial, OperatorTokenResponse,
    OperatorTokenStore, PolicyLayer, PolicyLayerKind, ProjectUsageSummary, Provider,
    ProviderHealth, ProviderHealthState, ProviderHealthStatus, ProviderIntelligenceStore, Route,
    ServiceImportDiff, ServiceRegistrySnapshot, StoredOperatorToken, UsageBreakdown,
    UsageBreakdownDimension, UsageEvent, UsageExport, UsageExportRow, UsageQuery, UsageQueryStore,
    UsageRecorder, UsageStatus, UsageSummary, UsageTimeseriesPoint, VirtualKeyMaterial,
};
use sqlx::{
    postgres::PgPoolOptions, types::Json, PgPool, Postgres, QueryBuilder, Row, Transaction,
};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;
use uuid::Uuid;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("postgres error: {0}")]
    Postgres(#[from] sqlx::Error),
    #[error("postgres migration error: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),
}

#[derive(Clone)]
pub struct PostgresStore {
    pool: PgPool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BudgetCounterSeed {
    pub key_id: Uuid,
    pub daily_spend_usd: f64,
    pub monthly_spend_usd: f64,
}

impl PostgresStore {
    pub async fn connect(database_url: &str) -> Result<Self, StoreError> {
        let pool = PgPoolOptions::new()
            .max_connections(10)
            .connect(database_url)
            .await?;
        MIGRATOR.run(&pool).await?;
        Ok(Self { pool })
    }

    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn has_active_operator_token(&self) -> GatewayResult<bool> {
        sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS (
                SELECT 1
                FROM operator_tokens
                WHERE disabled = false AND revoked_at IS NULL
            )
            "#,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|_| GatewayError::StoreUnavailable)
    }

    pub async fn ready(&self) -> Result<(), StoreError> {
        sqlx::query("SELECT 1").execute(&self.pool).await?;
        Ok(())
    }

    pub async fn budget_counter_seeds(
        &self,
        now: chrono::DateTime<chrono::Utc>,
    ) -> GatewayResult<Vec<BudgetCounterSeed>> {
        let (day_start, month_start) = budget_counter_windows(now)?;

        let rows = sqlx::query(
            r#"
            SELECT
                p.key_id,
                COALESCE(
                    SUM(u.estimated_cost) FILTER (
                        WHERE u.created_at >= $2
                    ),
                    0
                )::double precision AS daily_spend_usd,
                COALESCE(SUM(u.estimated_cost), 0)::double precision AS monthly_spend_usd
            FROM key_policies p
            INNER JOIN api_keys k ON k.id = p.key_id
            LEFT JOIN usage_events u
                ON u.key_id = p.key_id
               AND u.created_at >= $3
               AND u.estimated_cost IS NOT NULL
               AND u.estimated_cost > 0
            WHERE (p.daily_budget_usd IS NOT NULL OR p.monthly_budget_usd IS NOT NULL)
              AND k.disabled = false
              AND k.revoked_at IS NULL
              AND (k.expires_at IS NULL OR k.expires_at > $1)
            GROUP BY p.key_id
            "#,
        )
        .bind(now)
        .bind(day_start)
        .bind(month_start)
        .fetch_all(&self.pool)
        .await
        .map_err(|_| GatewayError::StoreUnavailable)?;

        rows.iter()
            .map(|row| {
                Ok(BudgetCounterSeed {
                    key_id: row.try_get("key_id")?,
                    daily_spend_usd: row.try_get("daily_spend_usd")?,
                    monthly_spend_usd: row.try_get("monthly_spend_usd")?,
                })
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()
            .map_err(|_| GatewayError::StoreUnavailable)
    }

    async fn upsert_policy_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        key_id: Uuid,
        policy: &KeyPolicy,
    ) -> GatewayResult<()> {
        sqlx::query(
            r#"
            INSERT INTO key_policies (
                key_id,
                deny,
                allowed_routes,
                allowed_models,
                allowed_providers,
                allowed_services,
                rpm_limit,
                tpm_limit,
                daily_budget_usd,
                monthly_budget_usd,
                allow_streaming,
                allow_tools,
                max_requests_per_day,
                max_tokens_per_day,
                max_cost_per_request,
                max_input_tokens_per_request,
                max_output_tokens_per_request,
                allowed_hours_utc,
                unused_key_auto_disable_after_days,
                max_request_body_bytes,
                max_response_body_bytes,
                max_stream_duration_seconds,
                max_sse_event_bytes,
                max_tool_call_count,
                max_tool_schema_bytes,
                policy_version
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25)
            ON CONFLICT (key_id) DO UPDATE SET
                deny = EXCLUDED.deny,
                allowed_routes = EXCLUDED.allowed_routes,
                allowed_models = EXCLUDED.allowed_models,
                allowed_providers = EXCLUDED.allowed_providers,
                allowed_services = EXCLUDED.allowed_services,
                rpm_limit = EXCLUDED.rpm_limit,
                tpm_limit = EXCLUDED.tpm_limit,
                daily_budget_usd = EXCLUDED.daily_budget_usd,
                monthly_budget_usd = EXCLUDED.monthly_budget_usd,
                allow_streaming = EXCLUDED.allow_streaming,
                allow_tools = EXCLUDED.allow_tools,
                max_requests_per_day = EXCLUDED.max_requests_per_day,
                max_tokens_per_day = EXCLUDED.max_tokens_per_day,
                max_cost_per_request = EXCLUDED.max_cost_per_request,
                max_input_tokens_per_request = EXCLUDED.max_input_tokens_per_request,
                max_output_tokens_per_request = EXCLUDED.max_output_tokens_per_request,
                allowed_hours_utc = EXCLUDED.allowed_hours_utc,
                unused_key_auto_disable_after_days = EXCLUDED.unused_key_auto_disable_after_days,
                max_request_body_bytes = EXCLUDED.max_request_body_bytes,
                max_response_body_bytes = EXCLUDED.max_response_body_bytes,
                max_stream_duration_seconds = EXCLUDED.max_stream_duration_seconds,
                max_sse_event_bytes = EXCLUDED.max_sse_event_bytes,
                max_tool_call_count = EXCLUDED.max_tool_call_count,
                max_tool_schema_bytes = EXCLUDED.max_tool_schema_bytes,
                policy_version = key_policies.policy_version + 1,
                updated_at = now()
            "#,
        )
        .bind(key_id)
        .bind(policy.deny)
        .bind(route_strings(&policy.allowed_routes))
        .bind(&policy.allowed_models)
        .bind(provider_strings(&policy.allowed_providers))
        .bind(&policy.allowed_services)
        .bind(policy.rpm_limit)
        .bind(policy.tpm_limit)
        .bind(policy.daily_budget_usd)
        .bind(policy.monthly_budget_usd)
        .bind(policy.allow_streaming)
        .bind(policy.allow_tools)
        .bind(policy.max_requests_per_day)
        .bind(policy.max_tokens_per_day)
        .bind(policy.max_cost_per_request)
        .bind(policy.max_input_tokens_per_request)
        .bind(policy.max_output_tokens_per_request)
        .bind(&policy.allowed_hours_utc)
        .bind(policy.unused_key_auto_disable_after_days)
        .bind(policy.max_request_body_bytes)
        .bind(policy.max_response_body_bytes)
        .bind(policy.max_stream_duration_seconds)
        .bind(policy.max_sse_event_bytes)
        .bind(policy.max_tool_call_count)
        .bind(policy.max_tool_schema_bytes)
        .bind(policy.policy_version.max(1))
        .execute(&mut **tx)
        .await
        .map_err(|_| GatewayError::StoreUnavailable)?;

        Ok(())
    }

    async fn upsert_guardrail_policy_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        key_id: Uuid,
        policy: &GuardrailPolicy,
    ) -> GatewayResult<()> {
        policy.validate()?;
        sqlx::query(
            r#"
            INSERT INTO key_guardrail_policies (
                key_id,
                mandatory_guardrails,
                optional_guardrails,
                forbidden_guardrails,
                guardrail_config_overrides
            )
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (key_id) DO UPDATE SET
                mandatory_guardrails = EXCLUDED.mandatory_guardrails,
                optional_guardrails = EXCLUDED.optional_guardrails,
                forbidden_guardrails = EXCLUDED.forbidden_guardrails,
                guardrail_config_overrides = EXCLUDED.guardrail_config_overrides,
                updated_at = now()
            "#,
        )
        .bind(key_id)
        .bind(&policy.mandatory_guardrails)
        .bind(&policy.optional_guardrails)
        .bind(&policy.forbidden_guardrails)
        .bind(Json(&policy.guardrail_config_overrides))
        .execute(&mut **tx)
        .await
        .map_err(|_| GatewayError::StoreUnavailable)?;
        Ok(())
    }

    async fn response_for_key(&self, key_id: Uuid) -> GatewayResult<Option<AdminKeyResponse>> {
        let Some(row) = sqlx::query(
            r#"
            SELECT
                k.id,
                k.owner_type,
                k.project_id,
                k.key_prefix,
                k.disabled,
                k.revoked_at,
                k.expires_at,
                k.rotation_due_at,
                k.last_used_at,
                k.created_at,
                k.updated_at,
                COALESCE(
                    ARRAY(
                        SELECT service_name
                        FROM key_service_links
                        WHERE key_id = k.id
                        ORDER BY service_name
                    ),
                    ARRAY[]::text[]
                ) AS service_names,
                p.allowed_routes,
                p.allowed_models,
                p.allowed_providers,
                p.allowed_services,
                p.deny,
                p.rpm_limit,
                p.tpm_limit,
                p.daily_budget_usd,
                p.monthly_budget_usd,
                p.allow_streaming,
                p.allow_tools,
                p.max_requests_per_day,
                p.max_tokens_per_day,
                p.max_cost_per_request,
                p.max_input_tokens_per_request,
                p.max_output_tokens_per_request,
                p.allowed_hours_utc,
                p.unused_key_auto_disable_after_days,
                p.max_request_body_bytes,
                p.max_response_body_bytes,
                p.max_stream_duration_seconds,
                p.max_sse_event_bytes,
                p.max_tool_call_count,
                p.max_tool_schema_bytes,
                p.policy_version,
                COALESCE(gp.mandatory_guardrails, ARRAY[]::text[]) AS mandatory_guardrails,
                COALESCE(gp.optional_guardrails, ARRAY[]::text[]) AS optional_guardrails,
                COALESCE(gp.forbidden_guardrails, ARRAY[]::text[]) AS forbidden_guardrails,
                COALESCE(gp.guardrail_config_overrides, '{}'::jsonb) AS guardrail_config_overrides
            FROM api_keys k
            LEFT JOIN key_policies p ON p.key_id = k.id
            LEFT JOIN key_guardrail_policies gp ON gp.key_id = k.id
            WHERE k.id = $1
            "#,
        )
        .bind(key_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| GatewayError::StoreUnavailable)?
        else {
            return Ok(None);
        };

        admin_key_response_from_row(&row).map(Some)
    }

    async fn validate_guardrail_policy_catalog(
        &self,
        policy: &GuardrailPolicy,
    ) -> GatewayResult<()> {
        policy.validate()?;
        let names = referenced_guardrail_policy_names(policy);
        if names.is_empty() {
            return Ok(());
        }
        let known_count = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)::bigint
            FROM guardrail_definitions
            WHERE name = ANY($1)
            "#,
        )
        .bind(&names)
        .fetch_one(&self.pool)
        .await
        .map_err(|_| GatewayError::StoreUnavailable)?;
        if known_count != i64::try_from(names.len()).unwrap_or(i64::MAX) {
            return Err(GatewayError::InvalidGuardrailRequest);
        }
        Ok(())
    }

    async fn policy_layers_for_context(
        &self,
        project_id: Option<Uuid>,
        team_id: Option<String>,
        route: Option<Route>,
        model: Option<String>,
        fallback_policy: &KeyPolicy,
    ) -> GatewayResult<Vec<PolicyLayer>> {
        let mut rows = sqlx::query(
            r#"
            SELECT layer_kind, scope_id, policy, guardrail_policy, policy_version
            FROM policy_layers
            WHERE (layer_kind = 'global' AND scope_id IS NULL)
               OR (layer_kind = 'project' AND scope_id = $1)
               OR (layer_kind = 'team' AND scope_id = $2)
               OR (layer_kind = 'route' AND scope_id = $3)
               OR (layer_kind = 'model' AND scope_id = $4)
            ORDER BY CASE layer_kind
                WHEN 'global' THEN 1
                WHEN 'project' THEN 2
                WHEN 'team' THEN 3
                WHEN 'route' THEN 5
                WHEN 'model' THEN 6
                ELSE 99
            END
            "#,
        )
        .bind(project_id.map(|id| id.to_string()))
        .bind(team_id)
        .bind(route.map(|route| route.as_str().to_owned()))
        .bind(model)
        .fetch_all(&self.pool)
        .await
        .map_err(|_| GatewayError::StoreUnavailable)?
        .into_iter()
        .map(|row| {
            let kind: String = row
                .try_get("layer_kind")
                .map_err(|_| GatewayError::StoreUnavailable)?;
            let policy: Json<KeyPolicy> = row
                .try_get("policy")
                .map_err(|_| GatewayError::StoreUnavailable)?;
            let guardrail_policy: Json<GuardrailPolicy> = row
                .try_get("guardrail_policy")
                .map_err(|_| GatewayError::StoreUnavailable)?;
            Ok(PolicyLayer {
                kind: parse_policy_layer_kind(&kind)?,
                scope_id: row.try_get("scope_id").ok().flatten(),
                policy: policy.0,
                guardrail_policy: guardrail_policy.0,
                policy_version: row.try_get("policy_version").unwrap_or(1),
            })
        })
        .collect::<GatewayResult<Vec<_>>>()?;

        if rows.is_empty() {
            rows.push(PolicyLayer {
                kind: PolicyLayerKind::Global,
                scope_id: None,
                policy: KeyPolicy {
                    policy_version: fallback_policy.policy_version,
                    ..KeyPolicy::default()
                },
                guardrail_policy: GuardrailPolicy::default(),
                policy_version: fallback_policy.policy_version,
            });
        }
        Ok(rows)
    }

    async fn stored_operator_token_by_prefix(
        &self,
        prefix: &str,
    ) -> GatewayResult<Option<StoredOperatorToken>> {
        sqlx::query(
            r#"
            SELECT id, token_prefix, token_hash, roles, scopes, disabled, revoked_at
            FROM operator_tokens
            WHERE token_prefix = $1
            "#,
        )
        .bind(prefix)
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| GatewayError::StoreUnavailable)?
        .map(|row| stored_operator_token_from_row(&row))
        .transpose()
        .map_err(|_| GatewayError::StoreUnavailable)
    }

    async fn stored_policy_for_key(&self, key_id: Uuid) -> GatewayResult<KeyPolicy> {
        let Some(row) = sqlx::query(
            r#"
            SELECT
                COALESCE(p.allowed_routes, ARRAY['/v1/chat/completions', '/v1/responses']::text[]) AS allowed_routes,
                COALESCE(p.allowed_models, ARRAY[]::text[]) AS allowed_models,
                COALESCE(p.allowed_providers, ARRAY['litellm']::text[]) AS allowed_providers,
                COALESCE(p.allowed_services, ARRAY[]::text[]) AS allowed_services,
                p.rpm_limit,
                p.tpm_limit,
                p.daily_budget_usd,
                p.monthly_budget_usd,
                COALESCE(p.allow_streaming, false) AS allow_streaming,
                COALESCE(p.allow_tools, false) AS allow_tools,
                COALESCE(p.deny, false) AS deny,
                p.max_requests_per_day,
                p.max_tokens_per_day,
                p.max_cost_per_request,
                p.max_input_tokens_per_request,
                p.max_output_tokens_per_request,
                COALESCE(p.allowed_hours_utc, ARRAY[]::integer[]) AS allowed_hours_utc,
                p.unused_key_auto_disable_after_days,
                p.max_request_body_bytes,
                p.max_response_body_bytes,
                p.max_stream_duration_seconds,
                p.max_sse_event_bytes,
                p.max_tool_call_count,
                p.max_tool_schema_bytes,
                COALESCE(p.policy_version, 1) AS policy_version
            FROM api_keys k
            LEFT JOIN key_policies p ON p.key_id = k.id
            WHERE k.id = $1
            "#,
        )
        .bind(key_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| GatewayError::StoreUnavailable)?
        else {
            return Ok(KeyPolicy::default());
        };

        let allowed_routes: Vec<String> = row
            .try_get("allowed_routes")
            .map_err(|_| GatewayError::StoreUnavailable)?;
        let allowed_models: Vec<String> = row
            .try_get("allowed_models")
            .map_err(|_| GatewayError::StoreUnavailable)?;
        let allowed_providers: Vec<String> = row
            .try_get("allowed_providers")
            .map_err(|_| GatewayError::StoreUnavailable)?;
        let allowed_services: Vec<String> = row
            .try_get("allowed_services")
            .map_err(|_| GatewayError::StoreUnavailable)?;
        Ok(KeyPolicy {
            deny: row.try_get("deny").unwrap_or(false),
            allowed_routes: parse_routes(&allowed_routes)?,
            allowed_models,
            allowed_providers: parse_providers(&allowed_providers)?,
            allowed_services,
            rpm_limit: row.try_get("rpm_limit").ok().flatten(),
            tpm_limit: row.try_get("tpm_limit").ok().flatten(),
            daily_budget_usd: row.try_get("daily_budget_usd").ok().flatten(),
            monthly_budget_usd: row.try_get("monthly_budget_usd").ok().flatten(),
            allow_streaming: row.try_get("allow_streaming").unwrap_or(false),
            allow_tools: row.try_get("allow_tools").unwrap_or(false),
            max_requests_per_day: row.try_get("max_requests_per_day").ok().flatten(),
            max_tokens_per_day: row.try_get("max_tokens_per_day").ok().flatten(),
            max_cost_per_request: row.try_get("max_cost_per_request").ok().flatten(),
            max_input_tokens_per_request: row
                .try_get("max_input_tokens_per_request")
                .ok()
                .flatten(),
            max_output_tokens_per_request: row
                .try_get("max_output_tokens_per_request")
                .ok()
                .flatten(),
            allowed_hours_utc: row.try_get("allowed_hours_utc").unwrap_or_default(),
            unused_key_auto_disable_after_days: row
                .try_get("unused_key_auto_disable_after_days")
                .ok()
                .flatten(),
            max_request_body_bytes: row.try_get("max_request_body_bytes").ok().flatten(),
            max_response_body_bytes: row.try_get("max_response_body_bytes").ok().flatten(),
            max_stream_duration_seconds: row.try_get("max_stream_duration_seconds").ok().flatten(),
            max_sse_event_bytes: row.try_get("max_sse_event_bytes").ok().flatten(),
            max_tool_call_count: row.try_get("max_tool_call_count").ok().flatten(),
            max_tool_schema_bytes: row.try_get("max_tool_schema_bytes").ok().flatten(),
            policy_version: row.try_get("policy_version").unwrap_or(1),
        })
    }

    async fn apply_daily_cap_denials(
        &self,
        key_id: Uuid,
        policy: &mut KeyPolicy,
    ) -> GatewayResult<()> {
        if policy.max_requests_per_day.is_none() && policy.max_tokens_per_day.is_none() {
            return Ok(());
        }
        let (daily_requests, daily_tokens) = sqlx::query_as::<_, (i64, i64)>(
            r#"
            SELECT COUNT(*)::bigint, COALESCE(SUM(total_tokens), 0)::bigint
            FROM usage_events
            WHERE key_id = $1
              AND created_at >= date_trunc('day', now())
            "#,
        )
        .bind(key_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|_| GatewayError::ControlStateUnavailable)?;
        if policy
            .max_requests_per_day
            .is_some_and(|limit| daily_requests >= i64::from(limit))
            || policy
                .max_tokens_per_day
                .is_some_and(|limit| daily_tokens >= i64::from(limit))
        {
            policy.deny = true;
        }
        Ok(())
    }

    async fn upsert_studio_service(
        &self,
        request: StudioServiceImportRequest,
    ) -> GatewayResult<ServiceResponse> {
        request.validate()?;
        let route_pattern = request
            .route_pattern
            .clone()
            .or_else(|| default_route_pattern(&request.name))
            .unwrap_or_else(|| format!("/services/{}/*", request.name));
        let cost_mode = request
            .default_pricing
            .as_ref()
            .map(|pricing| pricing.cost_mode)
            .unwrap_or_default();
        let estimated_cost_usd = request
            .default_pricing
            .as_ref()
            .and_then(|pricing| pricing.estimated_cost_usd);

        sqlx::query(
            r#"
            INSERT INTO service_registrations (
                name,
                project_id,
                studio_service_id,
                route_pattern,
                upstream_base_url,
                enabled,
                allowed_methods,
                cost_mode,
                estimated_cost_usd,
                source,
                sync_status,
                last_synced_at
            )
            VALUES ($1, $2, $3, $4, $5, false, $6, $7, $8, 'studio', 'incomplete', now())
            ON CONFLICT (studio_service_id) WHERE studio_service_id IS NOT NULL
            DO UPDATE SET
                name = EXCLUDED.name,
                studio_service_id = EXCLUDED.studio_service_id,
                source = 'studio',
                sync_status = CASE
                    WHEN service_registrations.upstream_base_url IS NULL
                        OR service_registrations.credential_secret IS NULL
                    THEN 'incomplete'
                    ELSE 'synced'
                END,
                last_synced_at = now(),
                updated_at = now()
            "#,
        )
        .bind(&request.name)
        .bind(request.project_id)
        .bind(&request.studio_service_id)
        .bind(&route_pattern)
        .bind(&request.upstream_base_url)
        .bind(&request.allowed_methods)
        .bind(service_cost_mode_str(cost_mode))
        .bind(estimated_cost_usd)
        .execute(&self.pool)
        .await
        .map_err(|error| {
            if is_unique_violation(&error) {
                GatewayError::DuplicateService
            } else if is_foreign_key_violation(&error) {
                GatewayError::MissingProject
            } else {
                GatewayError::StoreUnavailable
            }
        })?;

        self.get_service(&request.name)
            .await?
            .ok_or(GatewayError::StoreUnavailable)
    }

    async fn replace_project_service_links(
        &self,
        project_id: Uuid,
        service_names: &[String],
    ) -> GatewayResult<()> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| GatewayError::StoreUnavailable)?;
        sqlx::query("DELETE FROM project_service_links WHERE project_id = $1")
            .bind(project_id)
            .execute(&mut *tx)
            .await
            .map_err(|_| GatewayError::StoreUnavailable)?;
        for service_name in service_names {
            gateway_core::validate_service_name(service_name)?;
            sqlx::query(
                r#"
                INSERT INTO project_service_links (project_id, service_name)
                VALUES ($1, $2)
                ON CONFLICT DO NOTHING
                "#,
            )
            .bind(project_id)
            .bind(service_name)
            .execute(&mut *tx)
            .await
            .map_err(|error| {
                if is_foreign_key_violation(&error) {
                    GatewayError::MissingService
                } else {
                    GatewayError::StoreUnavailable
                }
            })?;
        }
        tx.commit()
            .await
            .map_err(|_| GatewayError::StoreUnavailable)
    }

    async fn replace_key_service_links_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        key_id: Uuid,
        service_names: &[String],
    ) -> GatewayResult<()> {
        sqlx::query("DELETE FROM key_service_links WHERE key_id = $1")
            .bind(key_id)
            .execute(&mut **tx)
            .await
            .map_err(|_| GatewayError::StoreUnavailable)?;
        for service_name in service_names {
            gateway_core::validate_service_name(service_name)?;
            sqlx::query(
                r#"
                INSERT INTO key_service_links (key_id, service_name)
                VALUES ($1, $2)
                ON CONFLICT DO NOTHING
                "#,
            )
            .bind(key_id)
            .bind(service_name)
            .execute(&mut **tx)
            .await
            .map_err(|error| {
                if is_foreign_key_violation(&error) {
                    GatewayError::MissingService
                } else {
                    GatewayError::StoreUnavailable
                }
            })?;
        }
        Ok(())
    }

    pub async fn insert_usage_event(&self, event: &UsageEvent) -> GatewayResult<()> {
        let route = event.route.as_str();
        let provider = event.provider.as_str();
        let status = match event.status {
            UsageStatus::Success => "success",
            UsageStatus::Failure => "failure",
        };

        sqlx::query(
            r#"
            INSERT INTO usage_events (
                request_id,
                key_id,
                project_id,
                route,
                model,
                provider,
                status,
                status_code,
                latency_ms,
                input_tokens,
                output_tokens,
                total_tokens,
                estimated_cost,
                service_name,
                task_id,
                run_id,
                fallback_count,
                created_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18)
            "#,
        )
        .bind(&event.request_id)
        .bind(event.key_id)
        .bind(event.project_id)
        .bind(route)
        .bind(&event.model)
        .bind(provider)
        .bind(status)
        .bind(i32::from(event.status_code))
        .bind(event.latency_ms)
        .bind(event.input_tokens)
        .bind(event.output_tokens)
        .bind(event.total_tokens)
        .bind(event.estimated_cost_usd)
        .bind(&event.service_name)
        .bind(&event.task_id)
        .bind(&event.run_id)
        .bind(event.fallback_count)
        .bind(event.created_at)
        .execute(&self.pool)
        .await
        .map_err(|_| GatewayError::StoreUnavailable)?;

        Ok(())
    }
}

fn budget_counter_windows(
    now: chrono::DateTime<chrono::Utc>,
) -> GatewayResult<(chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>)> {
    let day_start = now
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .map(|value| chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(value, chrono::Utc))
        .ok_or(GatewayError::StoreUnavailable)?;
    let month_start = chrono::NaiveDate::from_ymd_opt(now.year(), now.month(), 1)
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .map(|value| chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(value, chrono::Utc))
        .ok_or(GatewayError::StoreUnavailable)?;
    Ok((day_start, month_start))
}

#[async_trait]
impl UsageRecorder for PostgresStore {
    async fn insert_usage_event(&self, event: &UsageEvent) -> GatewayResult<()> {
        PostgresStore::insert_usage_event(self, event).await
    }
}

#[async_trait]
impl VirtualKeyLookup for PostgresStore {
    async fn find_by_prefix(&self, prefix: &str) -> GatewayResult<Option<StoredVirtualKey>> {
        sqlx::query(
            r#"
            UPDATE api_keys k
            SET disabled = true, updated_at = now()
            FROM key_policies p
            WHERE p.key_id = k.id
              AND k.key_prefix = $1
              AND k.disabled = false
              AND k.revoked_at IS NULL
              AND p.unused_key_auto_disable_after_days IS NOT NULL
              AND COALESCE(k.last_used_at, k.created_at)
                    <= now() - make_interval(days => p.unused_key_auto_disable_after_days)
            "#,
        )
        .bind(prefix)
        .execute(&self.pool)
        .await
        .map_err(|_| GatewayError::StoreUnavailable)?;

        sqlx::query_as::<
            _,
            (
                uuid::Uuid,
                Option<uuid::Uuid>,
                String,
                String,
                bool,
                Option<chrono::DateTime<chrono::Utc>>,
                Option<chrono::DateTime<chrono::Utc>>,
            ),
        >(
            r#"
            SELECT id, project_id, key_prefix, key_hash, disabled, revoked_at, expires_at
            FROM api_keys
            WHERE key_prefix = $1
            "#,
        )
        .bind(prefix)
        .fetch_optional(&self.pool)
        .await
        .map(|row| {
            row.map(
                |(id, project_id, key_prefix, key_hash, disabled, revoked_at, expires_at)| {
                    StoredVirtualKey {
                        id,
                        project_id,
                        key_prefix,
                        key_hash,
                        disabled,
                        revoked_at,
                        expires_at,
                    }
                },
            )
        })
        .map_err(|_| GatewayError::StoreUnavailable)
    }

    async fn mark_key_used(
        &self,
        key_id: Uuid,
        used_at: chrono::DateTime<chrono::Utc>,
    ) -> GatewayResult<()> {
        sqlx::query(
            r#"
            UPDATE api_keys
            SET last_used_at = $2, updated_at = now()
            WHERE id = $1
              AND revoked_at IS NULL
            "#,
        )
        .bind(key_id)
        .bind(used_at)
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|_| GatewayError::StoreUnavailable)
    }
}

#[async_trait]
impl gateway_core::PolicyLookup for PostgresStore {
    async fn policy_for_key(&self, key_id: Uuid) -> GatewayResult<KeyPolicy> {
        self.policy_for_context(key_id, None, None, None, None)
            .await
    }

    async fn effective_policy_for_context(
        &self,
        key_id: Uuid,
        project_id: Option<Uuid>,
        team_id: Option<String>,
        route: Option<Route>,
        model: Option<String>,
    ) -> GatewayResult<gateway_core::EffectivePolicy> {
        let policy = self
            .policy_for_context(key_id, project_id, team_id.clone(), route, model.clone())
            .await?;
        let mut guardrail_layers = self
            .policy_layers_for_context(
                project_id,
                team_id,
                route,
                model,
                &KeyPolicy::neutral_layer(policy.policy_version),
            )
            .await?;
        guardrail_layers.push(PolicyLayer {
            kind: PolicyLayerKind::Key,
            scope_id: Some(key_id.to_string()),
            policy: KeyPolicy::neutral_layer(policy.policy_version),
            guardrail_policy: self
                .guardrail_policy_for_key(key_id)
                .await
                .unwrap_or_default(),
            policy_version: policy.policy_version,
        });
        let effective_guardrails = resolve_effective_policy(guardrail_layers)?;
        Ok(gateway_core::EffectivePolicy {
            policy,
            guardrail_policy: effective_guardrails.guardrail_policy,
            applied_layers: effective_guardrails.applied_layers,
        })
    }

    async fn policy_for_context(
        &self,
        key_id: Uuid,
        context_project_id: Option<Uuid>,
        team_id: Option<String>,
        route: Option<Route>,
        model: Option<String>,
    ) -> GatewayResult<KeyPolicy> {
        let Some(row) = sqlx::query(
            r#"
            SELECT
                k.owner_type,
                k.project_id,
                COALESCE(p.allowed_routes, ARRAY['/v1/chat/completions', '/v1/responses']::text[]),
                COALESCE(p.allowed_models, ARRAY[]::text[]),
                COALESCE(p.allowed_providers, ARRAY['litellm']::text[]),
                COALESCE(p.allowed_services, ARRAY[]::text[]),
                COALESCE(
                    ARRAY(
                        SELECT service_name
                        FROM project_service_links
                        WHERE project_id = k.project_id
                        ORDER BY service_name
                    ),
                    ARRAY[]::text[]
                ) AS project_services,
                COALESCE(
                    ARRAY(
                        SELECT service_name
                        FROM key_service_links
                        WHERE key_id = k.id
                        ORDER BY service_name
                    ),
                    ARRAY[]::text[]
                ) AS key_services,
                p.rpm_limit,
                p.tpm_limit,
                p.daily_budget_usd,
                p.monthly_budget_usd,
                COALESCE(p.allow_streaming, false),
                COALESCE(p.allow_tools, false),
                COALESCE(p.deny, false),
                p.max_requests_per_day,
                p.max_tokens_per_day,
                p.max_cost_per_request,
                p.max_input_tokens_per_request,
                p.max_output_tokens_per_request,
                COALESCE(p.allowed_hours_utc, ARRAY[]::integer[]),
                p.unused_key_auto_disable_after_days,
                p.max_request_body_bytes,
                p.max_response_body_bytes,
                p.max_stream_duration_seconds,
                p.max_sse_event_bytes,
                p.max_tool_call_count,
                p.max_tool_schema_bytes,
                COALESCE(p.policy_version, 1)
            FROM api_keys k
            LEFT JOIN key_policies p ON p.key_id = k.id
            WHERE k.id = $1
            "#,
        )
        .bind(key_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| GatewayError::ControlStateUnavailable)?
        else {
            return Ok(KeyPolicy::default());
        };

        let owner_type: String = row
            .try_get("owner_type")
            .map_err(|_| GatewayError::ControlStateUnavailable)?;
        let stored_project_id: Option<Uuid> = row
            .try_get("project_id")
            .map_err(|_| GatewayError::ControlStateUnavailable)?;
        let project_id = context_project_id.or(stored_project_id);
        let allowed_routes: Vec<String> = row
            .try_get("allowed_routes")
            .map_err(|_| GatewayError::ControlStateUnavailable)?;
        let allowed_models: Vec<String> = row
            .try_get("allowed_models")
            .map_err(|_| GatewayError::ControlStateUnavailable)?;
        let allowed_providers: Vec<String> = row
            .try_get("allowed_providers")
            .map_err(|_| GatewayError::ControlStateUnavailable)?;
        let allowed_services: Vec<String> = row
            .try_get("allowed_services")
            .map_err(|_| GatewayError::ControlStateUnavailable)?;
        let project_services: Vec<String> = row.try_get("project_services").unwrap_or_default();
        let key_services: Vec<String> = row.try_get("key_services").unwrap_or_default();
        let rpm_limit: Option<i32> = row.try_get("rpm_limit").ok().flatten();
        let tpm_limit: Option<i32> = row.try_get("tpm_limit").ok().flatten();
        let daily_budget_usd: Option<f64> = row.try_get("daily_budget_usd").ok().flatten();
        let monthly_budget_usd: Option<f64> = row.try_get("monthly_budget_usd").ok().flatten();
        let allow_streaming: bool = row.try_get("allow_streaming").unwrap_or(false);
        let allow_tools: bool = row.try_get("allow_tools").unwrap_or(false);
        let deny: bool = row.try_get("deny").unwrap_or(false);
        let max_requests_per_day: Option<i32> = row.try_get("max_requests_per_day").ok().flatten();
        let max_tokens_per_day: Option<i32> = row.try_get("max_tokens_per_day").ok().flatten();
        let max_cost_per_request: Option<f64> = row.try_get("max_cost_per_request").ok().flatten();
        let max_input_tokens_per_request: Option<i32> =
            row.try_get("max_input_tokens_per_request").ok().flatten();
        let max_output_tokens_per_request: Option<i32> =
            row.try_get("max_output_tokens_per_request").ok().flatten();
        let allowed_hours_utc: Vec<i32> = row.try_get("allowed_hours_utc").unwrap_or_default();
        let unused_key_auto_disable_after_days: Option<i32> = row
            .try_get("unused_key_auto_disable_after_days")
            .ok()
            .flatten();
        let max_request_body_bytes: Option<i64> =
            row.try_get("max_request_body_bytes").ok().flatten();
        let max_response_body_bytes: Option<i64> =
            row.try_get("max_response_body_bytes").ok().flatten();
        let max_stream_duration_seconds: Option<i32> =
            row.try_get("max_stream_duration_seconds").ok().flatten();
        let max_sse_event_bytes: Option<i64> = row.try_get("max_sse_event_bytes").ok().flatten();
        let max_tool_call_count: Option<i32> = row.try_get("max_tool_call_count").ok().flatten();
        let max_tool_schema_bytes: Option<i64> =
            row.try_get("max_tool_schema_bytes").ok().flatten();
        let policy_version: i64 = row.try_get("policy_version").unwrap_or(1);

        let derived_services = match owner_type.as_str() {
            "project" if !project_services.is_empty() => project_services,
            "individual" if !key_services.is_empty() => key_services,
            _ => allowed_services,
        };
        let mut derived_routes = allowed_routes;
        if !derived_services.is_empty() {
            let service_route_patterns = sqlx::query_scalar::<_, String>(
                r#"
                SELECT route_pattern
                FROM service_registrations
                WHERE name = ANY($1)
                "#,
            )
            .bind(&derived_services)
            .fetch_all(&self.pool)
            .await
            .map_err(|_| GatewayError::ControlStateUnavailable)?;
            for route_pattern in service_route_patterns {
                let policy_route = service_route_policy_route(&route_pattern);
                if !derived_routes.iter().any(|route| route == policy_route) {
                    derived_routes.push(policy_route.to_owned());
                }
            }
        }

        let key_policy = KeyPolicy {
            deny,
            allowed_routes: parse_routes(&derived_routes)?,
            allowed_models,
            allowed_providers: parse_providers(&allowed_providers)?,
            allowed_services: derived_services,
            rpm_limit,
            tpm_limit,
            daily_budget_usd,
            monthly_budget_usd,
            allow_streaming,
            allow_tools,
            max_requests_per_day,
            max_tokens_per_day,
            max_cost_per_request,
            max_input_tokens_per_request,
            max_output_tokens_per_request,
            allowed_hours_utc,
            unused_key_auto_disable_after_days,
            max_request_body_bytes,
            max_response_body_bytes,
            max_stream_duration_seconds,
            max_sse_event_bytes,
            max_tool_call_count,
            max_tool_schema_bytes,
            policy_version,
        };
        let mut layers = self
            .policy_layers_for_context(project_id, team_id, route, model, &key_policy)
            .await?;
        layers.push(PolicyLayer {
            kind: PolicyLayerKind::Key,
            scope_id: Some(key_id.to_string()),
            policy: key_policy,
            guardrail_policy: self
                .guardrail_policy_for_key(key_id)
                .await
                .unwrap_or_default(),
            policy_version,
        });
        let mut effective_policy = resolve_effective_policy(layers)?.policy;
        self.apply_daily_cap_denials(key_id, &mut effective_policy)
            .await?;
        Ok(effective_policy)
    }
}

#[async_trait]
impl GuardrailStore for PostgresStore {
    async fn list_guardrail_definitions(&self) -> GatewayResult<Vec<GuardrailDefinition>> {
        let rows = sqlx::query(
            r#"
            SELECT name, description, modes, default_on, failure_policy, config_schema, config, enabled
            FROM guardrail_definitions
            WHERE enabled = true
            ORDER BY name ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|_| GatewayError::StoreUnavailable)?;

        rows.iter().map(guardrail_definition_from_row).collect()
    }

    async fn guardrail_policy_for_key(&self, key_id: Uuid) -> GatewayResult<GuardrailPolicy> {
        let row = sqlx::query(
            r#"
            SELECT mandatory_guardrails, optional_guardrails, forbidden_guardrails, guardrail_config_overrides
            FROM key_guardrail_policies
            WHERE key_id = $1
            "#,
        )
        .bind(key_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| GatewayError::StoreUnavailable)?;

        row.as_ref()
            .map(guardrail_policy_from_row)
            .transpose()
            .map(|policy| policy.unwrap_or_default())
    }

    async fn upsert_guardrail_policy_for_key(
        &self,
        key_id: Uuid,
        policy: &GuardrailPolicy,
    ) -> GatewayResult<()> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| GatewayError::StoreUnavailable)?;
        Self::upsert_guardrail_policy_in_tx(&mut tx, key_id, policy).await?;
        tx.commit()
            .await
            .map_err(|_| GatewayError::StoreUnavailable)
    }

    async fn insert_guardrail_execution_event(
        &self,
        event: &GuardrailExecutionEvent,
    ) -> GatewayResult<()> {
        sqlx::query(
            r#"
            INSERT INTO guardrail_execution_events (
                request_id,
                key_id,
                project_id,
                route,
                model,
                provider,
                guardrail_name,
                mode,
                action,
                failure_policy,
                latency_ms,
                reason,
                metadata,
                created_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
            "#,
        )
        .bind(&event.request_id)
        .bind(event.key_id)
        .bind(event.project_id)
        .bind(event.route.map(Route::as_str))
        .bind(&event.model)
        .bind(event.provider.map(Provider::as_str))
        .bind(&event.guardrail_name)
        .bind(event.mode.as_str())
        .bind(event.action.as_str())
        .bind(event.failure_policy.as_str())
        .bind(event.latency_ms)
        .bind(&event.reason)
        .bind(Json(&event.metadata))
        .bind(event.created_at)
        .execute(&self.pool)
        .await
        .map_err(|_| GatewayError::StoreUnavailable)?;
        Ok(())
    }
}

#[async_trait]
impl GuardrailObservabilityStore for PostgresStore {
    async fn list_admin_guardrail_definitions(
        &self,
    ) -> GatewayResult<Vec<AdminGuardrailDefinitionResponse>> {
        let rows = sqlx::query(
            r#"
            SELECT name, description, modes, default_on, failure_policy, config_schema, config, enabled
            FROM guardrail_definitions
            ORDER BY name ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|_| GatewayError::StoreUnavailable)?;

        rows.iter()
            .map(admin_guardrail_definition_from_row)
            .collect()
    }

    async fn guardrail_execution_events(
        &self,
        query: GuardrailEventQuery,
    ) -> GatewayResult<Vec<GuardrailExecutionEvent>> {
        let mut builder = QueryBuilder::<Postgres>::new(
            r#"
            SELECT request_id, key_id, project_id, route, model, provider, guardrail_name,
                   mode, action, failure_policy, latency_ms, reason, metadata, created_at
            FROM guardrail_execution_events
            "#,
        );
        append_guardrail_event_filters(&mut builder, &query);
        builder.push(" ORDER BY created_at DESC");
        builder.push(" LIMIT ");
        builder.push_bind(query.limit.unwrap_or(100).clamp(1, 500));
        builder.push(" OFFSET ");
        builder.push_bind(query.offset.unwrap_or(0).max(0));

        builder
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(|_| GatewayError::StoreUnavailable)?
            .iter()
            .map(guardrail_execution_event_from_row)
            .collect()
    }

    async fn guardrail_execution_summary(
        &self,
        query: GuardrailEventQuery,
    ) -> GatewayResult<Vec<GuardrailExecutionSummary>> {
        let mut builder = QueryBuilder::<Postgres>::new(
            r#"
            SELECT guardrail_name, mode, action, failure_policy,
                   COUNT(*)::bigint AS count,
                   COALESCE(SUM(latency_ms), 0)::bigint AS total_latency_ms
            FROM guardrail_execution_events
            "#,
        );
        append_guardrail_event_filters(&mut builder, &query);
        builder.push(
            " GROUP BY guardrail_name, mode, action, failure_policy ORDER BY guardrail_name, mode, action",
        );

        builder
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(|_| GatewayError::StoreUnavailable)?
            .iter()
            .map(guardrail_summary_from_row)
            .collect()
    }

    async fn create_http_guardrail(
        &self,
        request: GuardrailAdminCreateRequest,
    ) -> GatewayResult<AdminGuardrailDefinitionResponse> {
        ensure_json_object(&request.runtime_config)?;
        let modes = if request.modes.is_empty() {
            vec![GuardrailMode::PreCall, GuardrailMode::PostCall]
        } else {
            request.modes
        };
        let row = sqlx::query(
            r#"
            INSERT INTO guardrail_definitions (
                name, description, modes, default_on, failure_policy, config_schema, config, enabled
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            RETURNING name, description, modes, default_on, failure_policy, config_schema, config, enabled
            "#,
        )
        .bind(request.name.trim())
        .bind(request.description.trim())
        .bind(mode_strings(&modes))
        .bind(request.default_on)
        .bind(request.failure_policy.as_str())
        .bind(Json(&request.config_schema))
        .bind(Json(&serde_json::json!({
            "guardrail_name": request.name.trim(),
            "provider_kind": "http",
            "endpoint_url": request.endpoint_url.trim(),
            "timeout_ms": request.timeout_ms.unwrap_or(1500).clamp(100, 10_000),
            "bearer_token_secret": request.bearer_token,
            "provider_config": request.runtime_config
        })))
        .bind(request.enabled)
        .fetch_one(&self.pool)
        .await
        .map_err(|error| {
            if is_unique_violation(&error) {
                GatewayError::InvalidGuardrailRequest
            } else {
                GatewayError::StoreUnavailable
            }
        })?;
        admin_guardrail_definition_from_row(&row)
    }

    async fn patch_admin_guardrail(
        &self,
        name: String,
        request: GuardrailAdminPatchRequest,
    ) -> GatewayResult<AdminGuardrailDefinitionResponse> {
        let current = sqlx::query(
            r#"
            SELECT name, description, modes, default_on, failure_policy, config_schema, config, enabled
            FROM guardrail_definitions
            WHERE name = $1
            "#,
        )
        .bind(&name)
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| GatewayError::StoreUnavailable)?
        .ok_or(GatewayError::InvalidGuardrailRequest)?;
        let mut definition = guardrail_definition_from_row(&current)?;
        let is_http = definition
            .config
            .get("provider_kind")
            .and_then(serde_json::Value::as_str)
            == Some("http");
        if let Some(runtime_config) = request.runtime_config.as_ref() {
            ensure_json_object(runtime_config)?;
        }
        if !is_http
            && (request.description.is_some()
                || request.endpoint_url.is_some()
                || request.timeout_ms.is_some()
                || request.bearer_token.is_some())
        {
            return Err(GatewayError::InvalidGuardrailRequest);
        }
        let mut config = definition.config.as_object().cloned().unwrap_or_default();
        if let Some(value) = request.description {
            definition.description = value;
        }
        if let Some(value) = request.modes {
            definition.modes = value;
        }
        if let Some(value) = request.default_on {
            definition.default_on = value;
        }
        if let Some(value) = request.failure_policy {
            definition.failure_policy = value;
        }
        if let Some(value) = request.config_schema {
            definition.config_schema = value;
        }
        if let Some(value) = request.runtime_config {
            if is_http {
                config.insert("provider_config".to_owned(), value);
            } else {
                definition.config = value;
            }
        }
        if let Some(value) = request.enabled {
            definition.enabled = value;
        }
        if is_http {
            if let Some(value) = request.endpoint_url {
                config.insert("endpoint_url".to_owned(), serde_json::Value::String(value));
            }
        }
        if is_http {
            if let Some(value) = request.timeout_ms {
                config.insert(
                    "timeout_ms".to_owned(),
                    serde_json::Value::Number(serde_json::Number::from(value.clamp(100, 10_000))),
                );
            }
        }
        if is_http {
            if let Some(value) = request.bearer_token {
                config.insert(
                    "bearer_token_secret".to_owned(),
                    value
                        .map(serde_json::Value::String)
                        .unwrap_or(serde_json::Value::Null),
                );
            }
        }
        if is_http {
            definition.config = serde_json::Value::Object(config);
        }

        let row = sqlx::query(
            r#"
            UPDATE guardrail_definitions
            SET description = $2,
                modes = $3,
                default_on = $4,
                failure_policy = $5,
                config_schema = $6,
                config = $7,
                enabled = $8,
                updated_at = now()
            WHERE name = $1
            RETURNING name, description, modes, default_on, failure_policy, config_schema, config, enabled
            "#,
        )
        .bind(&name)
        .bind(&definition.description)
        .bind(mode_strings(&definition.modes))
        .bind(definition.default_on)
        .bind(definition.failure_policy.as_str())
        .bind(Json(&definition.config_schema))
        .bind(Json(&definition.config))
        .bind(definition.enabled)
        .fetch_one(&self.pool)
        .await
        .map_err(|_| GatewayError::StoreUnavailable)?;
        admin_guardrail_definition_from_row(&row)
    }

    async fn delete_admin_guardrail(&self, name: String) -> GatewayResult<()> {
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|_| GatewayError::StoreUnavailable)?;
        let current = sqlx::query(
            r#"
            SELECT config
            FROM guardrail_definitions
            WHERE name = $1
            FOR UPDATE
            "#,
        )
        .bind(&name)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(|_| GatewayError::StoreUnavailable)?
        .ok_or(GatewayError::InvalidGuardrailRequest)?;
        let config: serde_json::Value = current
            .try_get("config")
            .map_err(|_| GatewayError::StoreUnavailable)?;
        if config
            .get("provider_kind")
            .and_then(serde_json::Value::as_str)
            != Some("http")
        {
            return Err(GatewayError::InvalidGuardrailRequest);
        }

        sqlx::query("DELETE FROM guardrail_definitions WHERE name = $1")
            .bind(&name)
            .execute(&mut *transaction)
            .await
            .map_err(|_| GatewayError::StoreUnavailable)?;

        sqlx::query(
            r#"
            UPDATE key_guardrail_policies
            SET mandatory_guardrails = array_remove(mandatory_guardrails, $1),
                optional_guardrails = array_remove(optional_guardrails, $1),
                forbidden_guardrails = array_remove(forbidden_guardrails, $1),
                guardrail_config_overrides = guardrail_config_overrides - $1,
                updated_at = now()
            WHERE $1 = ANY(mandatory_guardrails)
               OR $1 = ANY(optional_guardrails)
               OR $1 = ANY(forbidden_guardrails)
               OR guardrail_config_overrides ? $1
            "#,
        )
        .bind(&name)
        .execute(&mut *transaction)
        .await
        .map_err(|_| GatewayError::StoreUnavailable)?;

        transaction
            .commit()
            .await
            .map_err(|_| GatewayError::StoreUnavailable)?;
        Ok(())
    }
}

#[async_trait]
impl AdminProjectStore for PostgresStore {
    async fn create_project(
        &self,
        request: ProjectCreateRequest,
    ) -> GatewayResult<ProjectResponse> {
        request.validate()?;
        let row = sqlx::query(
            r#"
            INSERT INTO projects (name)
            VALUES ($1)
            RETURNING id, name, created_at, updated_at, ARRAY[]::text[] AS service_names
            "#,
        )
        .bind(request.name.trim())
        .fetch_one(&self.pool)
        .await
        .map_err(|error| {
            if is_unique_violation(&error) {
                GatewayError::DuplicateProject
            } else {
                GatewayError::StoreUnavailable
            }
        })?;
        project_response_from_row(row)
    }

    async fn list_projects(&self) -> GatewayResult<Vec<ProjectResponse>> {
        sqlx::query(
            r#"
            SELECT
                p.id,
                p.name,
                p.created_at,
                p.updated_at,
                COALESCE(
                    ARRAY(
                        SELECT service_name
                        FROM project_service_links
                        WHERE project_id = p.id
                        ORDER BY service_name
                    ),
                    ARRAY[]::text[]
                ) AS service_names
            FROM projects p
            ORDER BY p.name ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map(|rows| {
            rows.into_iter()
                .map(project_response_from_row)
                .collect::<GatewayResult<Vec<_>>>()
        })
        .map_err(|_| GatewayError::StoreUnavailable)
        .and_then(|value| value)
    }

    async fn get_project(&self, project_id: Uuid) -> GatewayResult<Option<ProjectResponse>> {
        sqlx::query(
            r#"
            SELECT
                p.id,
                p.name,
                p.created_at,
                p.updated_at,
                COALESCE(
                    ARRAY(
                        SELECT service_name
                        FROM project_service_links
                        WHERE project_id = p.id
                        ORDER BY service_name
                    ),
                    ARRAY[]::text[]
                ) AS service_names
            FROM projects p
            WHERE p.id = $1
            "#,
        )
        .bind(project_id)
        .fetch_optional(&self.pool)
        .await
        .map(|row| row.map(project_response_from_row).transpose())
        .map_err(|_| GatewayError::StoreUnavailable)
        .and_then(|value| value)
    }

    async fn patch_project(
        &self,
        project_id: Uuid,
        patch: ProjectPatchRequest,
    ) -> GatewayResult<Option<ProjectResponse>> {
        patch.validate()?;
        if let Some(name) = patch.name {
            let rows = sqlx::query(
                r#"
                UPDATE projects
                SET name = $2, updated_at = now()
                WHERE id = $1
                "#,
            )
            .bind(project_id)
            .bind(name.trim())
            .execute(&self.pool)
            .await
            .map_err(|error| {
                if is_unique_violation(&error) {
                    GatewayError::DuplicateProject
                } else {
                    GatewayError::StoreUnavailable
                }
            })?
            .rows_affected();
            if rows == 0 {
                return Ok(None);
            }
        } else if self.get_project(project_id).await?.is_none() {
            return Ok(None);
        }
        if let Some(service_names) = patch.service_names {
            self.replace_project_service_links(project_id, &service_names)
                .await?;
        }
        self.get_project(project_id).await
    }

    async fn delete_project(&self, project_id: Uuid) -> GatewayResult<bool> {
        sqlx::query("DELETE FROM projects WHERE id = $1")
            .bind(project_id)
            .execute(&self.pool)
            .await
            .map(|result| result.rows_affected() > 0)
            .map_err(|error| {
                if is_foreign_key_violation(&error) {
                    GatewayError::ProjectInUse
                } else {
                    GatewayError::StoreUnavailable
                }
            })
    }
}

#[async_trait]
impl AdminKeyStore for PostgresStore {
    async fn create_admin_key(
        &self,
        request: AdminKeyCreate,
        material: &VirtualKeyMaterial,
    ) -> GatewayResult<AdminKeyResponse> {
        let key_id = Uuid::new_v4();
        validate_key_owner(request.owner_type, request.project_id)?;
        let base_policy = request
            .preset
            .map(|preset| preset.apply(KeyPolicy::default()))
            .unwrap_or_default();
        let policy = apply_policy_patch(base_policy, request.policy)?;
        self.validate_guardrail_policy_catalog(&request.guardrail_policy)
            .await?;
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| GatewayError::StoreUnavailable)?;

        sqlx::query(
            r#"
            INSERT INTO api_keys (id, owner_type, project_id, key_prefix, key_hash, expires_at, rotation_due_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
        )
        .bind(key_id)
        .bind(key_owner_type_str(request.owner_type))
        .bind(request.project_id)
        .bind(&material.key_prefix)
        .bind(&material.key_hash)
        .bind(request.expires_at)
        .bind(request.rotation_due_at)
        .execute(&mut *tx)
        .await
        .map_err(|error| {
            if is_foreign_key_violation(&error) {
                GatewayError::MissingProject
            } else {
                GatewayError::StoreUnavailable
            }
        })?;

        Self::upsert_policy_in_tx(&mut tx, key_id, &policy).await?;
        Self::upsert_guardrail_policy_in_tx(&mut tx, key_id, &request.guardrail_policy).await?;
        Self::replace_key_service_links_in_tx(&mut tx, key_id, &request.service_names).await?;
        tx.commit()
            .await
            .map_err(|_| GatewayError::StoreUnavailable)?;
        self.response_for_key(key_id)
            .await?
            .ok_or(GatewayError::StoreUnavailable)
    }

    async fn list_admin_keys(&self) -> GatewayResult<Vec<AdminKeyResponse>> {
        let rows = sqlx::query(
            r#"
            SELECT
                k.id,
                k.owner_type,
                k.project_id,
                k.key_prefix,
                k.disabled,
                k.revoked_at,
                k.expires_at,
                k.rotation_due_at,
                k.last_used_at,
                k.created_at,
                k.updated_at,
                COALESCE(
                    ARRAY(
                        SELECT service_name
                        FROM key_service_links
                        WHERE key_id = k.id
                        ORDER BY service_name
                    ),
                    ARRAY[]::text[]
                ) AS service_names,
                p.allowed_routes,
                p.allowed_models,
                p.allowed_providers,
                p.allowed_services,
                p.deny,
                p.rpm_limit,
                p.tpm_limit,
                p.daily_budget_usd,
                p.monthly_budget_usd,
                p.allow_streaming,
                p.allow_tools,
                p.max_requests_per_day,
                p.max_tokens_per_day,
                p.max_cost_per_request,
                p.max_input_tokens_per_request,
                p.max_output_tokens_per_request,
                p.allowed_hours_utc,
                p.unused_key_auto_disable_after_days,
                p.max_request_body_bytes,
                p.max_response_body_bytes,
                p.max_stream_duration_seconds,
                p.max_sse_event_bytes,
                p.max_tool_call_count,
                p.max_tool_schema_bytes,
                p.policy_version,
                COALESCE(gp.mandatory_guardrails, ARRAY[]::text[]) AS mandatory_guardrails,
                COALESCE(gp.optional_guardrails, ARRAY[]::text[]) AS optional_guardrails,
                COALESCE(gp.forbidden_guardrails, ARRAY[]::text[]) AS forbidden_guardrails,
                COALESCE(gp.guardrail_config_overrides, '{}'::jsonb) AS guardrail_config_overrides
            FROM api_keys k
            LEFT JOIN key_policies p ON p.key_id = k.id
            LEFT JOIN key_guardrail_policies gp ON gp.key_id = k.id
            ORDER BY k.created_at DESC, k.id DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|_| GatewayError::StoreUnavailable)?;

        rows.iter()
            .map(admin_key_response_from_row)
            .collect::<GatewayResult<Vec<_>>>()
    }

    async fn get_admin_key(&self, key_id: Uuid) -> GatewayResult<Option<AdminKeyResponse>> {
        self.response_for_key(key_id).await
    }

    async fn patch_admin_key(
        &self,
        key_id: Uuid,
        patch: AdminKeyPatch,
    ) -> GatewayResult<Option<AdminKeyResponse>> {
        let update_expires_at = patch.expires_at.is_some();
        let expires_at = patch.expires_at.flatten();
        let update_rotation_due_at = patch.rotation_due_at.is_some();
        let rotation_due_at = patch.rotation_due_at.flatten();
        let owner_type = patch.owner_type;
        let project_id = patch.project_id.flatten();
        let update_project_id = patch.project_id.is_some();
        if owner_type.is_some() || update_project_id {
            let Some(current) = self.response_for_key(key_id).await? else {
                return Ok(None);
            };
            validate_key_owner(
                owner_type.unwrap_or(current.owner_type),
                if update_project_id {
                    project_id
                } else {
                    current.project_id
                },
            )?;
        }

        let policy = if let Some(policy_patch) = patch.policy {
            let current = self.stored_policy_for_key(key_id).await?;
            Some(apply_policy_patch(current, policy_patch)?)
        } else {
            None
        };
        let guardrail_policy = if let Some(guardrail_patch) = patch.guardrail_policy {
            let current = self.guardrail_policy_for_key(key_id).await?;
            let policy = guardrail_patch.apply(current)?;
            self.validate_guardrail_policy_catalog(&policy).await?;
            Some(policy)
        } else {
            None
        };

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| GatewayError::StoreUnavailable)?;

        let rows = sqlx::query(
            r#"
            UPDATE api_keys
            SET
                owner_type = COALESCE($5, owner_type),
                project_id = CASE WHEN $6 THEN $7 ELSE project_id END,
                expires_at = CASE WHEN $2 THEN $3 ELSE expires_at END,
                disabled = COALESCE($4, disabled),
                rotation_due_at = CASE WHEN $8 THEN $9 ELSE rotation_due_at END,
                updated_at = now()
            WHERE id = $1
              AND revoked_at IS NULL
            "#,
        )
        .bind(key_id)
        .bind(update_expires_at)
        .bind(expires_at)
        .bind(patch.disabled)
        .bind(owner_type.map(key_owner_type_str))
        .bind(update_project_id)
        .bind(project_id)
        .bind(update_rotation_due_at)
        .bind(rotation_due_at)
        .execute(&mut *tx)
        .await
        .map_err(|error| {
            if is_foreign_key_violation(&error) {
                GatewayError::MissingProject
            } else {
                GatewayError::StoreUnavailable
            }
        })?
        .rows_affected();

        if rows == 0 {
            tx.rollback()
                .await
                .map_err(|_| GatewayError::StoreUnavailable)?;
            return self.response_for_key(key_id).await;
        }

        if let Some(policy) = policy {
            Self::upsert_policy_in_tx(&mut tx, key_id, &policy).await?;
        }
        if let Some(policy) = guardrail_policy {
            Self::upsert_guardrail_policy_in_tx(&mut tx, key_id, &policy).await?;
        }
        if let Some(service_names) = patch.service_names {
            Self::replace_key_service_links_in_tx(&mut tx, key_id, &service_names).await?;
        }

        tx.commit()
            .await
            .map_err(|_| GatewayError::StoreUnavailable)?;
        self.response_for_key(key_id).await
    }

    async fn revoke_admin_key(&self, key_id: Uuid) -> GatewayResult<Option<AdminKeyResponse>> {
        let rows = sqlx::query(
            r#"
            UPDATE api_keys
            SET disabled = true, revoked_at = now(), updated_at = now()
            WHERE id = $1
              AND revoked_at IS NULL
            "#,
        )
        .bind(key_id)
        .execute(&self.pool)
        .await
        .map_err(|_| GatewayError::StoreUnavailable)?
        .rows_affected();

        if rows == 0 {
            return self.response_for_key(key_id).await;
        }
        self.response_for_key(key_id).await
    }

    async fn disable_admin_key(&self, key_id: Uuid) -> GatewayResult<Option<AdminKeyResponse>> {
        let rows = sqlx::query(
            r#"
            UPDATE api_keys
            SET disabled = true, updated_at = now()
            WHERE id = $1
              AND revoked_at IS NULL
            "#,
        )
        .bind(key_id)
        .execute(&self.pool)
        .await
        .map_err(|_| GatewayError::StoreUnavailable)?
        .rows_affected();

        if rows == 0 {
            return self.response_for_key(key_id).await;
        }
        self.response_for_key(key_id).await
    }

    async fn enable_admin_key(&self, key_id: Uuid) -> GatewayResult<Option<AdminKeyResponse>> {
        let rows = sqlx::query(
            r#"
            UPDATE api_keys
            SET disabled = false, updated_at = now()
            WHERE id = $1
              AND revoked_at IS NULL
            "#,
        )
        .bind(key_id)
        .execute(&self.pool)
        .await
        .map_err(|_| GatewayError::StoreUnavailable)?
        .rows_affected();

        if rows == 0 {
            return self.response_for_key(key_id).await;
        }
        self.response_for_key(key_id).await
    }

    async fn key_usage_summary(&self, key_id: Uuid) -> GatewayResult<Option<AdminKeyUsageSummary>> {
        let key_exists = sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS (
                SELECT 1
                FROM api_keys
                WHERE id = $1
            )
            "#,
        )
        .bind(key_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|_| GatewayError::StoreUnavailable)?;
        if !key_exists {
            return Ok(None);
        }

        sqlx::query_as::<_, (i64, i64, i64, Option<i64>, i64, i64, i64, Option<f64>)>(
            r#"
            SELECT
                COUNT(*)::bigint,
                COUNT(*) FILTER (WHERE status = 'success')::bigint,
                COUNT(*) FILTER (WHERE status = 'failure')::bigint,
                COALESCE(SUM(latency_ms), 0)::bigint,
                COALESCE(SUM(input_tokens), 0)::bigint,
                COALESCE(SUM(output_tokens), 0)::bigint,
                COALESCE(SUM(total_tokens), 0)::bigint,
                COALESCE(SUM(estimated_cost), 0)::double precision
            FROM usage_events
            WHERE key_id = $1
            "#,
        )
        .bind(key_id)
        .fetch_one(&self.pool)
        .await
        .map(
            |(
                request_count,
                success_count,
                failure_count,
                total_latency_ms,
                input_tokens,
                output_tokens,
                total_tokens,
                estimated_cost_usd,
            )| {
                Some(AdminKeyUsageSummary {
                    key_id,
                    request_count,
                    success_count,
                    failure_count,
                    total_latency_ms: total_latency_ms.unwrap_or(0),
                    input_tokens,
                    output_tokens,
                    total_tokens,
                    estimated_cost_usd,
                })
            },
        )
        .map_err(|_| GatewayError::StoreUnavailable)
    }

    async fn project_usage_summary(&self, project_id: Uuid) -> GatewayResult<ProjectUsageSummary> {
        sqlx::query_as::<_, (i64, i64, i64, Option<i64>, i64, i64, i64, Option<f64>)>(
            r#"
            SELECT
                COUNT(*)::bigint,
                COUNT(*) FILTER (WHERE status = 'success')::bigint,
                COUNT(*) FILTER (WHERE status = 'failure')::bigint,
                COALESCE(SUM(latency_ms), 0)::bigint,
                COALESCE(SUM(input_tokens), 0)::bigint,
                COALESCE(SUM(output_tokens), 0)::bigint,
                COALESCE(SUM(total_tokens), 0)::bigint,
                COALESCE(SUM(estimated_cost), 0)::double precision
            FROM usage_events
            WHERE project_id = $1
            "#,
        )
        .bind(project_id)
        .fetch_one(&self.pool)
        .await
        .map(
            |(
                request_count,
                success_count,
                failure_count,
                total_latency_ms,
                input_tokens,
                output_tokens,
                total_tokens,
                estimated_cost_usd,
            )| {
                ProjectUsageSummary {
                    project_id,
                    request_count,
                    success_count,
                    failure_count,
                    total_latency_ms: total_latency_ms.unwrap_or(0),
                    input_tokens,
                    output_tokens,
                    total_tokens,
                    estimated_cost_usd,
                }
            },
        )
        .map_err(|_| GatewayError::StoreUnavailable)
    }
}

#[async_trait]
impl AdminPolicyLayerStore for PostgresStore {
    async fn list_policy_layers(&self) -> GatewayResult<Vec<AdminPolicyLayerResponse>> {
        let rows = sqlx::query(
            r#"
            SELECT id, layer_kind, scope_id, policy, guardrail_policy, policy_version, created_at, updated_at
            FROM policy_layers
            ORDER BY CASE layer_kind
                WHEN 'global' THEN 1
                WHEN 'project' THEN 2
                WHEN 'team' THEN 3
                WHEN 'key' THEN 4
                WHEN 'route' THEN 5
                WHEN 'model' THEN 6
                ELSE 99
            END, scope_id NULLS FIRST
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|_| GatewayError::StoreUnavailable)?;

        rows.iter()
            .map(policy_layer_response_from_row)
            .collect::<GatewayResult<Vec<_>>>()
    }

    async fn upsert_policy_layer(
        &self,
        request: AdminPolicyLayerUpsert,
    ) -> GatewayResult<AdminPolicyLayerResponse> {
        let scope_id = normalize_policy_layer_scope(request.kind, request.scope_id)?;
        let layer_kind = request.kind.as_str();
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| GatewayError::StoreUnavailable)?;
        let existing = sqlx::query(
            r#"
            SELECT id, policy, guardrail_policy, policy_version
            FROM policy_layers
            WHERE layer_kind = $1
              AND scope_id IS NOT DISTINCT FROM $2
            FOR UPDATE
            "#,
        )
        .bind(layer_kind)
        .bind(&scope_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| GatewayError::StoreUnavailable)?;

        let (layer_id, base_policy, base_guardrail_policy, next_version) =
            if let Some(row) = existing {
                let policy: Json<KeyPolicy> = row
                    .try_get("policy")
                    .map_err(|_| GatewayError::StoreUnavailable)?;
                let guardrail_policy: Json<GuardrailPolicy> = row
                    .try_get("guardrail_policy")
                    .map_err(|_| GatewayError::StoreUnavailable)?;
                let version = row.try_get::<i64, _>("policy_version").unwrap_or(1) + 1;
                (
                    row.try_get("id")
                        .map_err(|_| GatewayError::StoreUnavailable)?,
                    policy.0,
                    guardrail_policy.0,
                    version,
                )
            } else {
                (
                    Uuid::new_v4(),
                    KeyPolicy::neutral_layer(1),
                    GuardrailPolicy::default(),
                    1,
                )
            };

        let mut policy = apply_policy_patch(base_policy, request.policy)?;
        policy.policy_version = next_version;
        let guardrail_policy = request.guardrail_policy.apply(base_guardrail_policy)?;
        self.validate_guardrail_policy_catalog(&guardrail_policy)
            .await?;

        sqlx::query(
            r#"
            INSERT INTO policy_layers (
                id, layer_kind, scope_id, policy, guardrail_policy, policy_version
            )
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (id) DO UPDATE SET
                policy = EXCLUDED.policy,
                guardrail_policy = EXCLUDED.guardrail_policy,
                policy_version = EXCLUDED.policy_version,
                updated_at = now()
            "#,
        )
        .bind(layer_id)
        .bind(layer_kind)
        .bind(&scope_id)
        .bind(Json(&policy))
        .bind(Json(&guardrail_policy))
        .bind(next_version)
        .execute(&mut *tx)
        .await
        .map_err(|_| GatewayError::StoreUnavailable)?;
        tx.commit()
            .await
            .map_err(|_| GatewayError::StoreUnavailable)?;

        sqlx::query(
            r#"
            SELECT id, layer_kind, scope_id, policy, guardrail_policy, policy_version, created_at, updated_at
            FROM policy_layers
            WHERE id = $1
            "#,
        )
        .bind(layer_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|_| GatewayError::StoreUnavailable)
        .and_then(|row| policy_layer_response_from_row(&row))
    }

    async fn delete_policy_layer(&self, layer_id: Uuid) -> GatewayResult<bool> {
        sqlx::query("DELETE FROM policy_layers WHERE id = $1")
            .bind(layer_id)
            .execute(&self.pool)
            .await
            .map(|result| result.rows_affected() > 0)
            .map_err(|_| GatewayError::StoreUnavailable)
    }
}

#[async_trait]
impl OperatorTokenStore for PostgresStore {
    async fn bootstrap_operator_token(
        &self,
        material: &OperatorTokenMaterial,
    ) -> GatewayResult<Option<OperatorTokenResponse>> {
        let token_id = Uuid::new_v4();
        match sqlx::query(
            r#"
            INSERT INTO operator_tokens (id, token_prefix, token_hash)
            SELECT $1, $2, $3
            WHERE NOT EXISTS (
                SELECT 1
                FROM operator_tokens
                WHERE disabled = false AND revoked_at IS NULL
            )
            RETURNING id, token_prefix, roles, scopes, disabled, revoked_at, last_used_at, created_at, updated_at
            "#,
        )
        .bind(token_id)
        .bind(&material.token_prefix)
        .bind(&material.token_hash)
        .fetch_optional(&self.pool)
        .await
        {
            Ok(row) => Ok(row.map(operator_token_response_from_row)),
            Err(error) if is_unique_violation_on(&error, "operator_tokens_one_active_idx") => {
                Ok(None)
            }
            Err(_) => Err(GatewayError::StoreUnavailable),
        }
    }

    async fn verify_operator_token(
        &self,
        raw_token: &str,
        now: chrono::DateTime<chrono::Utc>,
    ) -> GatewayResult<OperatorAuthorization> {
        let prefix = operator_token_prefix(raw_token)?;
        let stored = self
            .stored_operator_token_by_prefix(&prefix)
            .await?
            .ok_or(GatewayError::InvalidOperatorToken)?;
        let authorization = verify_stored_operator_token(raw_token, &stored)?;

        sqlx::query(
            r#"
            UPDATE operator_tokens
            SET last_used_at = $2, updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(stored.id)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|_| GatewayError::StoreUnavailable)?;
        Ok(authorization)
    }

    async fn rotate_operator_token(
        &self,
        current_raw_token: &str,
        material: &OperatorTokenMaterial,
        now: chrono::DateTime<chrono::Utc>,
    ) -> GatewayResult<OperatorTokenResponse> {
        let prefix = operator_token_prefix(current_raw_token)?;
        let stored = self
            .stored_operator_token_by_prefix(&prefix)
            .await?
            .ok_or(GatewayError::InvalidOperatorToken)?;
        verify_stored_operator_token(current_raw_token, &stored)?;

        let token_id = Uuid::new_v4();
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| GatewayError::StoreUnavailable)?;
        sqlx::query(
            r#"
            UPDATE operator_tokens
            SET disabled = true, revoked_at = $2, updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(stored.id)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(|_| GatewayError::StoreUnavailable)?;
        let response = sqlx::query(
            r#"
            INSERT INTO operator_tokens (id, token_prefix, token_hash, roles, scopes)
            VALUES ($1, $2, $3, $4, $5)
            RETURNING id, token_prefix, roles, scopes, disabled, revoked_at, last_used_at, created_at, updated_at
            "#,
        )
        .bind(token_id)
        .bind(&material.token_prefix)
        .bind(&material.token_hash)
        .bind(&stored.roles)
        .bind(&stored.scopes)
        .fetch_one(&mut *tx)
        .await
        .map(operator_token_response_from_row)
        .map_err(|_| GatewayError::StoreUnavailable)?;
        tx.commit()
            .await
            .map_err(|_| GatewayError::StoreUnavailable)?;

        Ok(response)
    }
}

#[async_trait]
impl AdminAuditStore for PostgresStore {
    async fn record_audit_event(&self, event: AuditEventCreate) -> GatewayResult<AuditEvent> {
        sqlx::query(
            r#"
            INSERT INTO audit_events (
                actor_token_id,
                action,
                target_type,
                target_id,
                before_json,
                after_json,
                request_id,
                ip,
                user_agent
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            RETURNING
                id,
                actor_token_id,
                action,
                target_type,
                target_id,
                before_json,
                after_json,
                request_id,
                ip,
                user_agent,
                created_at
            "#,
        )
        .bind(event.actor_token_id)
        .bind(event.action)
        .bind(event.target_type)
        .bind(event.target_id)
        .bind(event.before.map(Json))
        .bind(event.after.map(Json))
        .bind(event.request_id)
        .bind(event.ip)
        .bind(event.user_agent)
        .fetch_one(&self.pool)
        .await
        .and_then(audit_event_from_row)
        .map_err(|_| GatewayError::StoreUnavailable)
    }

    async fn list_audit_events(&self, query: AuditEventQuery) -> GatewayResult<Vec<AuditEvent>> {
        let limit = query.limit.clamp(1, 500);
        let rows = sqlx::query(
            r#"
            SELECT
                id,
                actor_token_id,
                action,
                target_type,
                target_id,
                before_json,
                after_json,
                request_id,
                ip,
                user_agent,
                created_at
            FROM audit_events
            WHERE ($1::uuid IS NULL OR actor_token_id = $1)
              AND ($2::text IS NULL OR action = $2)
              AND ($3::text IS NULL OR target_type = $3)
              AND ($4::text IS NULL OR target_id = $4)
            ORDER BY created_at DESC
            LIMIT $5
            "#,
        )
        .bind(query.actor_token_id)
        .bind(query.action)
        .bind(query.target_type)
        .bind(query.target_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|_| GatewayError::StoreUnavailable)?;

        rows.into_iter()
            .map(audit_event_from_row)
            .collect::<Result<Vec<_>, sqlx::Error>>()
            .map_err(|_| GatewayError::StoreUnavailable)
    }
}

#[async_trait]
impl AdminOpenAiRouteStore for PostgresStore {
    async fn list_openai_route_settings(&self) -> GatewayResult<Vec<OpenAiRouteSetting>> {
        let rows = sqlx::query(
            r#"
            SELECT route_id, route, enabled, updated_at
            FROM openai_route_settings
            ORDER BY route_id
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|_| GatewayError::StoreUnavailable)?;

        rows.iter()
            .map(openai_route_setting_from_row)
            .collect::<GatewayResult<Vec<_>>>()
    }

    async fn set_openai_route_enabled(
        &self,
        route_id: &str,
        enabled: bool,
    ) -> GatewayResult<Option<OpenAiRouteSetting>> {
        if gateway_core::openai_route_from_id(route_id).is_none() {
            return Ok(None);
        }

        sqlx::query(
            r#"
            UPDATE openai_route_settings
            SET enabled = $2,
                updated_at = now()
            WHERE route_id = $1
            "#,
        )
        .bind(route_id)
        .bind(enabled)
        .execute(&self.pool)
        .await
        .map_err(|_| GatewayError::StoreUnavailable)?;

        sqlx::query(
            r#"
            SELECT route_id, route, enabled, updated_at
            FROM openai_route_settings
            WHERE route_id = $1
            "#,
        )
        .bind(route_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| GatewayError::StoreUnavailable)?
        .map(|row| openai_route_setting_from_row(&row))
        .transpose()
    }
}

#[async_trait]
impl OpenAiRouteSettingsLookup for PostgresStore {
    async fn openai_route_enabled(&self, route: Route) -> GatewayResult<bool> {
        let Some(route_id) = gateway_core::openai_route_id(route) else {
            return Ok(true);
        };

        sqlx::query_scalar::<_, bool>(
            r#"
            SELECT enabled
            FROM openai_route_settings
            WHERE route_id = $1
            "#,
        )
        .bind(route_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| GatewayError::StoreUnavailable)?
        .ok_or(GatewayError::StoreUnavailable)
    }
}

#[async_trait]
impl AdminProviderConfigStore for PostgresStore {
    async fn create_provider_config(
        &self,
        request: ProviderConfigCreateRequest,
    ) -> GatewayResult<ProviderConfigResponse> {
        request.validate()?;
        let row = sqlx::query(
            r#"
            INSERT INTO provider_configs (provider, name, base_url, enabled, credential_secret)
            VALUES ($1, $2, $3, $4, $5)
            RETURNING id, provider, name, base_url, enabled, credential_secret, created_at, updated_at
            "#,
        )
        .bind(provider_config_kind_str(request.provider))
        .bind(request.name.trim())
        .bind(request.base_url.trim())
        .bind(request.enabled)
        .bind(request.credential)
        .fetch_one(&self.pool)
        .await
        .map_err(|error| {
            if is_unique_violation(&error) {
                GatewayError::DuplicateProviderConfig
            } else {
                GatewayError::StoreUnavailable
            }
        })?;
        provider_config_response_from_row(&row)
    }

    async fn list_provider_configs(&self) -> GatewayResult<Vec<ProviderConfigResponse>> {
        sqlx::query(
            r#"
            SELECT id, provider, name, base_url, enabled, credential_secret, created_at, updated_at
            FROM provider_configs
            ORDER BY provider ASC, name ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map(|rows| {
            rows.into_iter()
                .map(|row| provider_config_response_from_row(&row))
                .collect::<GatewayResult<Vec<_>>>()
        })
        .map_err(|_| GatewayError::StoreUnavailable)?
    }

    async fn get_provider_config(
        &self,
        provider_id: Uuid,
    ) -> GatewayResult<Option<ProviderConfigResponse>> {
        sqlx::query(
            r#"
            SELECT id, provider, name, base_url, enabled, credential_secret, created_at, updated_at
            FROM provider_configs
            WHERE id = $1
            "#,
        )
        .bind(provider_id)
        .fetch_optional(&self.pool)
        .await
        .map(|row| {
            row.map(|row| provider_config_response_from_row(&row))
                .transpose()
        })
        .map_err(|_| GatewayError::StoreUnavailable)?
    }

    async fn patch_provider_config(
        &self,
        provider_id: Uuid,
        patch: ProviderConfigPatchRequest,
    ) -> GatewayResult<Option<ProviderConfigResponse>> {
        patch.validate()?;
        let Some(row) = sqlx::query(
            r#"
            SELECT id, provider, name, base_url, enabled, credential_secret, created_at, updated_at
            FROM provider_configs
            WHERE id = $1
            "#,
        )
        .bind(provider_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| GatewayError::StoreUnavailable)?
        else {
            return Ok(None);
        };
        let mut response = provider_config_response_from_row(&row)?;
        if let Some(name) = patch.name {
            response.name = name.trim().to_owned();
        }
        if let Some(base_url) = patch.base_url {
            response.base_url = base_url.trim().to_owned();
        }
        if let Some(enabled) = patch.enabled {
            response.enabled = enabled;
        }
        let credential_secret: Option<Option<String>> = patch.credential;

        sqlx::query(
            r#"
            UPDATE provider_configs
            SET name = $2,
                base_url = $3,
                enabled = $4,
                credential_secret = CASE WHEN $5::boolean THEN $6 ELSE credential_secret END,
                updated_at = now()
            WHERE id = $1
            RETURNING id, provider, name, base_url, enabled, credential_secret, created_at, updated_at
            "#,
        )
        .bind(provider_id)
        .bind(&response.name)
        .bind(&response.base_url)
        .bind(response.enabled)
        .bind(credential_secret.is_some())
        .bind(credential_secret.flatten())
        .fetch_optional(&self.pool)
        .await
        .map(|row| row.map(|row| provider_config_response_from_row(&row)).transpose())
        .map_err(|error| {
            if is_unique_violation(&error) {
                GatewayError::DuplicateProviderConfig
            } else {
                GatewayError::StoreUnavailable
            }
        })?
    }

    async fn delete_provider_config(&self, provider_id: Uuid) -> GatewayResult<bool> {
        sqlx::query("DELETE FROM provider_configs WHERE id = $1")
            .bind(provider_id)
            .execute(&self.pool)
            .await
            .map(|result| result.rows_affected() > 0)
            .map_err(|_| GatewayError::StoreUnavailable)
    }

    async fn set_provider_config_enabled(
        &self,
        provider_id: Uuid,
        enabled: bool,
    ) -> GatewayResult<Option<ProviderConfigResponse>> {
        sqlx::query(
            r#"
            UPDATE provider_configs
            SET enabled = $2, updated_at = now()
            WHERE id = $1
            RETURNING id, provider, name, base_url, enabled, credential_secret, created_at, updated_at
            "#,
        )
        .bind(provider_id)
        .bind(enabled)
        .fetch_optional(&self.pool)
        .await
        .map(|row| row.map(|row| provider_config_response_from_row(&row)).transpose())
        .map_err(|error| {
            if is_unique_violation(&error) {
                GatewayError::DuplicateProviderConfig
            } else {
                GatewayError::StoreUnavailable
            }
        })?
    }
}

#[async_trait]
impl ProviderConfigLookup for PostgresStore {
    async fn active_litellm_config(&self) -> GatewayResult<Option<ProviderRuntimeConfig>> {
        sqlx::query_as::<_, (String, String)>(
            r#"
            SELECT base_url, credential_secret
            FROM provider_configs
            WHERE provider = 'litellm'
              AND enabled
              AND credential_secret IS NOT NULL
            ORDER BY updated_at DESC
            LIMIT 1
            "#,
        )
        .fetch_optional(&self.pool)
        .await
        .map(|row| {
            row.map(|(base_url, credential)| ProviderRuntimeConfig {
                provider: Provider::LiteLlm,
                base_url,
                credential,
            })
        })
        .map_err(|_| GatewayError::StoreUnavailable)
    }
}

#[async_trait]
impl AdminStudioConnectionStore for PostgresStore {
    async fn studio_connection_settings(&self) -> GatewayResult<Option<StoredStudioConnection>> {
        sqlx::query(
            r#"
            SELECT base_url, bearer_token_secret, updated_at
            FROM studio_connection_settings
            WHERE singleton = true
            "#,
        )
        .fetch_optional(&self.pool)
        .await
        .map(|row| row.map(|row| studio_connection_from_row(&row)).transpose())
        .map_err(|_| GatewayError::StoreUnavailable)?
    }

    async fn patch_studio_connection_settings(
        &self,
        patch: StudioConnectionPatchRequest,
    ) -> GatewayResult<StoredStudioConnection> {
        patch.validate()?;
        let current = self.studio_connection_settings().await?.unwrap_or_default();

        let mut base_url = current.base_url;
        let mut bearer_token_secret = current.bearer_token_secret;

        match patch.base_url {
            PatchValue::Unchanged => {}
            PatchValue::Clear => {
                base_url = None;
                bearer_token_secret = None;
            }
            PatchValue::Set(value) => {
                base_url = Some(normalize_base_url(&value)?);
            }
        }

        match patch.token {
            PatchValue::Unchanged => {}
            PatchValue::Clear => {
                bearer_token_secret = None;
            }
            PatchValue::Set(value) => {
                bearer_token_secret = Some(normalize_secret(&value)?);
            }
        }

        let row = sqlx::query(
            r#"
            INSERT INTO studio_connection_settings (
                singleton,
                base_url,
                bearer_token_secret
            )
            VALUES (true, $1, $2)
            ON CONFLICT (singleton) DO UPDATE SET
                base_url = EXCLUDED.base_url,
                bearer_token_secret = EXCLUDED.bearer_token_secret,
                updated_at = now()
            RETURNING base_url, bearer_token_secret, updated_at
            "#,
        )
        .bind(base_url)
        .bind(bearer_token_secret)
        .fetch_one(&self.pool)
        .await
        .map_err(|_| GatewayError::StoreUnavailable)?;

        studio_connection_from_row(&row)
    }
}

#[async_trait]
impl AdminServiceStore for PostgresStore {
    async fn create_service(
        &self,
        request: ServiceCreateRequest,
    ) -> GatewayResult<ServiceResponse> {
        request.validate()?;
        let route_pattern = request
            .route_pattern
            .clone()
            .or_else(|| default_route_pattern(&request.name))
            .unwrap_or_else(|| format!("/services/{}/*", request.name));
        let sync_status = if request.studio_service_id.is_some() {
            service_sync_status_for_runtime(
                request.upstream_base_url.as_deref(),
                request.credential.as_deref(),
                ServiceSyncStatus::Synced,
            )
        } else {
            ServiceSyncStatus::Local
        };

        sqlx::query(
            r#"
            INSERT INTO service_registrations (
                name,
                project_id,
                studio_service_id,
                route_pattern,
                upstream_base_url,
                enabled,
                allowed_methods,
                timeout_ms,
                max_body_bytes,
                cost_mode,
                estimated_cost_usd,
                credential_secret,
                fallback_services,
                source,
                sync_status,
                last_synced_at,
                disabled_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, CASE WHEN $3 IS NULL THEN NULL ELSE now() END, CASE WHEN $6 THEN NULL ELSE now() END)
            "#,
        )
        .bind(&request.name)
        .bind(request.project_id)
        .bind(&request.studio_service_id)
        .bind(&route_pattern)
        .bind(&request.upstream_base_url)
        .bind(request.enabled)
        .bind(&request.allowed_methods)
        .bind(request.timeout_ms)
        .bind(request.max_body_bytes)
        .bind(service_cost_mode_str(request.cost_mode))
        .bind(request.estimated_cost_usd)
        .bind(&request.credential)
        .bind(&request.fallback_services)
        .bind(if request.studio_service_id.is_some() { "studio" } else { "gateway" })
        .bind(service_sync_status_str(sync_status))
        .execute(&self.pool)
        .await
        .map_err(|error| {
            if is_unique_violation(&error) {
                GatewayError::DuplicateService
            } else if is_foreign_key_violation(&error) {
                GatewayError::MissingProject
            } else {
                GatewayError::StoreUnavailable
            }
        })?;

        self.get_service(&request.name)
            .await?
            .ok_or(GatewayError::StoreUnavailable)
    }

    async fn list_services(&self) -> GatewayResult<Vec<ServiceResponse>> {
        sqlx::query("SELECT * FROM service_registrations ORDER BY name ASC")
            .fetch_all(&self.pool)
            .await
            .map(|rows| {
                rows.into_iter()
                    .map(|row| service_registration_from_row(&row).map(|row| row.to_response()))
                    .collect::<Result<Vec<_>, _>>()
            })
            .map_err(|_| GatewayError::StoreUnavailable)?
            .map_err(|_| GatewayError::StoreUnavailable)
    }

    async fn get_service(&self, name: &str) -> GatewayResult<Option<ServiceResponse>> {
        self.service_registration(name)
            .await
            .map(|registration| registration.map(|registration| registration.to_response()))
    }

    async fn patch_service(
        &self,
        name: &str,
        patch: ServicePatchRequest,
    ) -> GatewayResult<Option<ServiceResponse>> {
        gateway_core::validate_service_name(name)?;
        patch.validate()?;
        let Some(mut registration) = self.service_registration(name).await? else {
            return Ok(None);
        };

        if let Some(studio_service_id) = patch.studio_service_id {
            registration.studio_service_id = studio_service_id;
            registration.source = if registration.studio_service_id.is_some() {
                ServiceSource::Studio
            } else {
                ServiceSource::Gateway
            };
        }
        if let Some(project_id) = patch.project_id {
            registration.project_id = project_id;
        }
        if let Some(route_pattern) = patch.route_pattern {
            registration.route_pattern = route_pattern;
        }
        if let Some(upstream_base_url) = patch.upstream_base_url {
            registration.upstream_base_url = upstream_base_url;
        }
        if let Some(enabled) = patch.enabled {
            registration.enabled = enabled;
            registration.disabled_at = None;
        }
        if let Some(allowed_methods) = patch.allowed_methods {
            registration.allowed_methods = allowed_methods;
        }
        if let Some(credential) = patch.credential {
            registration.credential_secret = credential;
        }
        if let Some(timeout_ms) = patch.timeout_ms {
            registration.timeout_ms = timeout_ms;
        }
        if let Some(max_body_bytes) = patch.max_body_bytes {
            registration.max_body_bytes = max_body_bytes;
        }
        if let Some(cost_mode) = patch.cost_mode {
            registration.cost_mode = cost_mode;
        }
        if let Some(estimated_cost_usd) = patch.estimated_cost_usd {
            registration.estimated_cost_usd = estimated_cost_usd;
        }
        if let Some(fallback_services) = patch.fallback_services {
            registration.fallback_services = fallback_services;
        }
        registration.sync_status = patch.sync_status.unwrap_or_else(|| {
            if registration.source == ServiceSource::Studio {
                service_sync_status_for_runtime(
                    registration.upstream_base_url.as_deref(),
                    registration.credential_secret.as_deref(),
                    ServiceSyncStatus::Synced,
                )
            } else {
                ServiceSyncStatus::Local
            }
        });

        sqlx::query(
            r#"
            UPDATE service_registrations
            SET
                studio_service_id = $2,
                project_id = $3,
                route_pattern = $4,
                upstream_base_url = $5,
                enabled = $6,
                allowed_methods = $7,
                timeout_ms = $8,
                max_body_bytes = $9,
                cost_mode = $10,
                estimated_cost_usd = $11,
                credential_secret = $12,
                fallback_services = $13,
                source = $14,
                sync_status = $15,
                disabled_at = CASE WHEN $6 THEN NULL ELSE COALESCE(disabled_at, now()) END,
                updated_at = now()
            WHERE name = $1
            "#,
        )
        .bind(name)
        .bind(&registration.studio_service_id)
        .bind(registration.project_id)
        .bind(&registration.route_pattern)
        .bind(&registration.upstream_base_url)
        .bind(registration.enabled)
        .bind(&registration.allowed_methods)
        .bind(registration.timeout_ms)
        .bind(registration.max_body_bytes)
        .bind(service_cost_mode_str(registration.cost_mode))
        .bind(registration.estimated_cost_usd)
        .bind(&registration.credential_secret)
        .bind(&registration.fallback_services)
        .bind(service_source_str(registration.source))
        .bind(service_sync_status_str(registration.sync_status))
        .execute(&self.pool)
        .await
        .map_err(|error| {
            if is_unique_violation(&error) {
                GatewayError::DuplicateService
            } else if is_foreign_key_violation(&error) {
                GatewayError::MissingProject
            } else {
                GatewayError::StoreUnavailable
            }
        })?;

        self.get_service(name).await
    }

    async fn delete_service(&self, name: &str) -> GatewayResult<bool> {
        gateway_core::validate_service_name(name)?;
        sqlx::query("DELETE FROM service_registrations WHERE name = $1")
            .bind(name)
            .execute(&self.pool)
            .await
            .map(|result| result.rows_affected() > 0)
            .map_err(|_| GatewayError::StoreUnavailable)
    }

    async fn set_service_enabled(
        &self,
        name: &str,
        enabled: bool,
    ) -> GatewayResult<Option<ServiceResponse>> {
        gateway_core::validate_service_name(name)?;
        let rows = sqlx::query(
            r#"
            UPDATE service_registrations
            SET enabled = $2,
                disabled_at = CASE WHEN $2 THEN NULL ELSE COALESCE(disabled_at, now()) END,
                updated_at = now()
            WHERE name = $1
            "#,
        )
        .bind(name)
        .bind(enabled)
        .execute(&self.pool)
        .await
        .map_err(|_| GatewayError::StoreUnavailable)?
        .rows_affected();

        if rows == 0 {
            return Ok(None);
        }
        self.get_service(name).await
    }

    async fn import_studio_service(
        &self,
        request: StudioServiceImportRequest,
    ) -> GatewayResult<ServiceResponse> {
        self.upsert_studio_service(request).await
    }

    async fn sync_studio_service(
        &self,
        request: StudioServiceImportRequest,
    ) -> GatewayResult<ServiceResponse> {
        self.upsert_studio_service(request).await
    }

    async fn service_sync_status(
        &self,
        name: &str,
    ) -> GatewayResult<Option<ServiceSyncStatusResponse>> {
        self.service_registration(name).await.map(|registration| {
            registration.map(|registration| registration.sync_status_response())
        })
    }
}

#[async_trait]
impl ServiceRegistryLookup for PostgresStore {
    async fn service_registration(&self, name: &str) -> GatewayResult<Option<ServiceRegistration>> {
        gateway_core::validate_service_name(name)?;
        sqlx::query("SELECT * FROM service_registrations WHERE name = $1")
            .bind(name)
            .fetch_optional(&self.pool)
            .await
            .map(|row| {
                row.map(|row| service_registration_from_row(&row))
                    .transpose()
            })
            .map_err(|_| GatewayError::StoreUnavailable)?
            .map_err(|_| GatewayError::StoreUnavailable)
    }
}

#[async_trait]
impl ServiceRouteLookup for PostgresStore {
    async fn service_registration_for_route(
        &self,
        method: &http::Method,
        path: &str,
    ) -> GatewayResult<Option<ServiceRegistration>> {
        let rows =
            sqlx::query("SELECT * FROM service_registrations ORDER BY length(route_pattern) DESC")
                .fetch_all(&self.pool)
                .await
                .map_err(|_| GatewayError::StoreUnavailable)?;

        rows.into_iter()
            .map(|row| service_registration_from_row(&row))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| GatewayError::StoreUnavailable)
            .map(|services| {
                services.into_iter().find(|service| {
                    route_pattern_matches(&service.route_pattern, path)
                        && service
                            .allowed_methods
                            .iter()
                            .any(|allowed| allowed.eq_ignore_ascii_case(method.as_str()))
                })
            })
    }
}

#[async_trait]
impl UsageQueryStore for PostgresStore {
    async fn usage_summary(&self, query: UsageQuery) -> GatewayResult<UsageSummary> {
        let mut builder = QueryBuilder::<Postgres>::new(
            r#"
            SELECT
                COUNT(*)::bigint,
                COUNT(*) FILTER (WHERE status = 'success')::bigint,
                COUNT(*) FILTER (WHERE status = 'failure')::bigint,
                COALESCE(SUM(input_tokens), 0)::bigint,
                COALESCE(SUM(output_tokens), 0)::bigint,
                COALESCE(SUM(total_tokens), 0)::bigint,
                COALESCE(SUM(estimated_cost), 0)::double precision,
                COALESCE(SUM(latency_ms), 0)::bigint,
                COALESCE(SUM(fallback_count), 0)::bigint
            FROM usage_events
            "#,
        );
        append_usage_filters(&mut builder, &query);
        builder
            .build_query_as::<(i64, i64, i64, i64, i64, i64, Option<f64>, i64, i64)>()
            .fetch_one(&self.pool)
            .await
            .map(summary_from_row)
            .map_err(|_| GatewayError::StoreUnavailable)
    }

    async fn usage_timeseries(
        &self,
        query: UsageQuery,
    ) -> GatewayResult<Vec<UsageTimeseriesPoint>> {
        let interval = match query.interval.as_deref() {
            Some("day") => "day",
            _ => "hour",
        };
        let mut builder = QueryBuilder::<Postgres>::new("SELECT date_trunc(");
        builder.push_bind(interval);
        builder.push(
            r#", created_at) AS bucket,
                COUNT(*)::bigint,
                COUNT(*) FILTER (WHERE status = 'success')::bigint,
                COUNT(*) FILTER (WHERE status = 'failure')::bigint,
                COALESCE(SUM(input_tokens), 0)::bigint,
                COALESCE(SUM(output_tokens), 0)::bigint,
                COALESCE(SUM(total_tokens), 0)::bigint,
                COALESCE(SUM(estimated_cost), 0)::double precision,
                COALESCE(SUM(latency_ms), 0)::bigint,
                COALESCE(SUM(fallback_count), 0)::bigint
            FROM usage_events
            "#,
        );
        append_usage_filters(&mut builder, &query);
        builder.push(" GROUP BY bucket ORDER BY bucket ASC");

        builder
            .build_query_as::<(
                chrono::DateTime<chrono::Utc>,
                i64,
                i64,
                i64,
                i64,
                i64,
                i64,
                Option<f64>,
                i64,
                i64,
            )>()
            .fetch_all(&self.pool)
            .await
            .map(|rows| {
                rows.into_iter()
                    .map(
                        |(
                            bucket,
                            request_count,
                            success_count,
                            failure_count,
                            input_tokens,
                            output_tokens,
                            total_tokens,
                            estimated_cost_usd,
                            total_latency_ms,
                            fallback_count,
                        )| UsageTimeseriesPoint {
                            bucket,
                            summary: UsageSummary {
                                request_count,
                                success_count,
                                failure_count,
                                input_tokens,
                                output_tokens,
                                total_tokens,
                                estimated_cost_usd,
                                total_latency_ms,
                                fallback_count,
                            },
                        },
                    )
                    .collect()
            })
            .map_err(|_| GatewayError::StoreUnavailable)
    }

    async fn usage_breakdown(
        &self,
        query: UsageQuery,
        dimension: UsageBreakdownDimension,
    ) -> GatewayResult<Vec<UsageBreakdown>> {
        let column = match dimension {
            UsageBreakdownDimension::Key => "key_id::text",
            UsageBreakdownDimension::Project => "COALESCE(project_id::text, 'individual')",
            UsageBreakdownDimension::Model => "COALESCE(model, 'unknown')",
            UsageBreakdownDimension::Provider => "provider",
            UsageBreakdownDimension::Service => "COALESCE(service_name, 'none')",
            UsageBreakdownDimension::Task => "COALESCE(task_id, 'none')",
        };
        let mut builder = QueryBuilder::<Postgres>::new("SELECT ");
        builder.push(column);
        builder.push(
            r#" AS name,
                COUNT(*)::bigint,
                COUNT(*) FILTER (WHERE status = 'success')::bigint,
                COUNT(*) FILTER (WHERE status = 'failure')::bigint,
                COALESCE(SUM(input_tokens), 0)::bigint,
                COALESCE(SUM(output_tokens), 0)::bigint,
                COALESCE(SUM(total_tokens), 0)::bigint,
                COALESCE(SUM(estimated_cost), 0)::double precision,
                COALESCE(SUM(latency_ms), 0)::bigint,
                COALESCE(SUM(fallback_count), 0)::bigint
            FROM usage_events
            "#,
        );
        append_usage_filters(&mut builder, &query);
        builder.push(" GROUP BY name ORDER BY 2 DESC, name ASC");

        builder
            .build_query_as::<(String, i64, i64, i64, i64, i64, i64, Option<f64>, i64, i64)>()
            .fetch_all(&self.pool)
            .await
            .map(|rows| {
                rows.into_iter()
                    .map(
                        |(
                            name,
                            request_count,
                            success_count,
                            failure_count,
                            input_tokens,
                            output_tokens,
                            total_tokens,
                            estimated_cost_usd,
                            total_latency_ms,
                            fallback_count,
                        )| UsageBreakdown {
                            name,
                            summary: UsageSummary {
                                request_count,
                                success_count,
                                failure_count,
                                input_tokens,
                                output_tokens,
                                total_tokens,
                                estimated_cost_usd,
                                total_latency_ms,
                                fallback_count,
                            },
                        },
                    )
                    .collect()
            })
            .map_err(|_| GatewayError::StoreUnavailable)
    }

    async fn usage_export(&self, query: UsageQuery) -> GatewayResult<UsageExport> {
        let summary = self.usage_summary(query.clone()).await?;
        let mut builder = QueryBuilder::<Postgres>::new(
            r#"
            SELECT
                u.request_id,
                u.key_id,
                u.project_id,
                u.route,
                u.model,
                u.provider,
                u.status,
                u.status_code,
                u.latency_ms,
                COALESCE(u.input_tokens, 0)::bigint,
                COALESCE(u.output_tokens, 0)::bigint,
                COALESCE(u.total_tokens, 0)::bigint,
                u.estimated_cost::double precision,
                u.service_name,
                u.task_id,
                u.run_id,
                u.fallback_count,
                COALESCE(g.guardrail_action_count, 0)::bigint,
                u.created_at
            FROM usage_events u
            LEFT JOIN (
                SELECT request_id, COUNT(*)::bigint AS guardrail_action_count
                FROM guardrail_execution_events
                GROUP BY request_id
            ) g ON g.request_id = u.request_id
            "#,
        );
        append_usage_filters_with_alias(&mut builder, &query, "u");
        builder.push(" ORDER BY u.created_at ASC, u.request_id ASC");
        builder.push(" LIMIT ");
        builder.push_bind(query.limit.unwrap_or(1_000).clamp(1, 10_000));
        builder.push(" OFFSET ");
        builder.push_bind(query.offset.unwrap_or_default().max(0));

        let rows = builder
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(|_| GatewayError::StoreUnavailable)?
            .into_iter()
            .map(|row| {
                Ok(UsageExportRow {
                    request_id: row.try_get("request_id")?,
                    key_id: row.try_get("key_id")?,
                    project_id: row.try_get("project_id")?,
                    route: row.try_get("route")?,
                    model: row.try_get("model")?,
                    provider: row.try_get("provider")?,
                    status: row.try_get("status")?,
                    status_code: row.try_get("status_code")?,
                    latency_ms: row.try_get("latency_ms")?,
                    input_tokens: row.try_get("input_tokens")?,
                    output_tokens: row.try_get("output_tokens")?,
                    total_tokens: row.try_get("total_tokens")?,
                    estimated_cost_usd: row.try_get("estimated_cost_usd")?,
                    service_name: row.try_get("service_name")?,
                    task_id: row.try_get("task_id")?,
                    run_id: row.try_get("run_id")?,
                    fallback_count: row.try_get("fallback_count")?,
                    guardrail_action_count: row.try_get("guardrail_action_count")?,
                    created_at: row.try_get("created_at")?,
                })
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()
            .map_err(|_| GatewayError::StoreUnavailable)?;

        Ok(UsageExport { summary, rows })
    }

    async fn provider_health(&self, query: UsageQuery) -> GatewayResult<Vec<ProviderHealth>> {
        let mut builder = QueryBuilder::<Postgres>::new(
            r#"
            SELECT
                COALESCE(service_name, provider) AS name,
                COUNT(*)::bigint,
                COUNT(*) FILTER (WHERE status = 'failure')::bigint,
                COUNT(*) FILTER (WHERE status_code = 504)::bigint,
                COALESCE(SUM(fallback_count), 0)::bigint,
                COALESCE(SUM(latency_ms), 0)::bigint
            FROM usage_events
            "#,
        );
        append_usage_filters(&mut builder, &query);
        builder.push(" GROUP BY name ORDER BY name ASC");

        builder
            .build_query_as::<(String, i64, i64, i64, i64, i64)>()
            .fetch_all(&self.pool)
            .await
            .map(|rows| {
                rows.into_iter()
                    .map(
                        |(
                            name,
                            request_count,
                            error_count,
                            timeout_count,
                            fallback_count,
                            total_latency_ms,
                        )| ProviderHealth {
                            name,
                            request_count,
                            error_count,
                            timeout_count,
                            fallback_count,
                            total_latency_ms,
                        },
                    )
                    .collect()
            })
            .map_err(|_| GatewayError::StoreUnavailable)
    }
}

#[async_trait]
impl ProviderIntelligenceStore for PostgresStore {
    async fn list_provider_health_states(&self) -> GatewayResult<Vec<ProviderHealthState>> {
        sqlx::query(
            r#"
            SELECT
                name,
                provider,
                status,
                circuit_state,
                active_check_ok,
                passive_success_count,
                passive_failure_count,
                consecutive_failures,
                average_latency_ms,
                last_error_code,
                cooldown_until,
                checked_at,
                updated_at
            FROM provider_health_states
            ORDER BY name ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map(|rows| {
            rows.into_iter()
                .map(|row| provider_health_state_from_row(&row))
                .collect::<GatewayResult<Vec<_>>>()
        })
        .map_err(|_| GatewayError::StoreUnavailable)?
    }

    async fn upsert_provider_health_state(
        &self,
        state: ProviderHealthState,
    ) -> GatewayResult<ProviderHealthState> {
        let row = sqlx::query(
            r#"
            INSERT INTO provider_health_states (
                name,
                provider,
                status,
                circuit_state,
                active_check_ok,
                passive_success_count,
                passive_failure_count,
                consecutive_failures,
                average_latency_ms,
                last_error_code,
                cooldown_until,
                checked_at,
                updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, now())
            ON CONFLICT (name) DO UPDATE SET
                provider = EXCLUDED.provider,
                status = EXCLUDED.status,
                circuit_state = EXCLUDED.circuit_state,
                active_check_ok = EXCLUDED.active_check_ok,
                passive_success_count = EXCLUDED.passive_success_count,
                passive_failure_count = EXCLUDED.passive_failure_count,
                consecutive_failures = EXCLUDED.consecutive_failures,
                average_latency_ms = EXCLUDED.average_latency_ms,
                last_error_code = EXCLUDED.last_error_code,
                cooldown_until = EXCLUDED.cooldown_until,
                checked_at = EXCLUDED.checked_at,
                updated_at = now()
            RETURNING
                name,
                provider,
                status,
                circuit_state,
                active_check_ok,
                passive_success_count,
                passive_failure_count,
                consecutive_failures,
                average_latency_ms,
                last_error_code,
                cooldown_until,
                checked_at,
                updated_at
            "#,
        )
        .bind(&state.name)
        .bind(provider_str(state.provider))
        .bind(provider_health_status_str(state.status))
        .bind(circuit_state_str(state.circuit_state))
        .bind(state.active_check_ok)
        .bind(state.passive_success_count)
        .bind(state.passive_failure_count)
        .bind(state.consecutive_failures)
        .bind(state.average_latency_ms)
        .bind(&state.last_error_code)
        .bind(state.cooldown_until)
        .bind(state.checked_at)
        .fetch_one(&self.pool)
        .await
        .map_err(|_| GatewayError::StoreUnavailable)?;

        provider_health_state_from_row(&row)
    }

    async fn get_debug_bundle(&self, request_id: &str) -> GatewayResult<Option<DebugBundle>> {
        sqlx::query(
            r#"
            SELECT
                request_id,
                route,
                provider,
                service_name,
                policy_trace,
                guardrail_trace,
                selection_trace,
                fallback_history,
                upstream_latency_ms,
                request_hash,
                response_hash,
                redaction_version,
                created_at
            FROM request_debug_bundles
            WHERE request_id = $1
            "#,
        )
        .bind(request_id)
        .fetch_optional(&self.pool)
        .await
        .map(|row| row.map(|row| debug_bundle_from_row(&row)).transpose())
        .map_err(|_| GatewayError::StoreUnavailable)?
    }

    async fn insert_debug_bundle(&self, bundle: DebugBundle) -> GatewayResult<()> {
        sqlx::query(
            r#"
            INSERT INTO request_debug_bundles (
                request_id,
                route,
                provider,
                service_name,
                policy_trace,
                guardrail_trace,
                selection_trace,
                fallback_history,
                upstream_latency_ms,
                request_hash,
                response_hash,
                redaction_version,
                created_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
            ON CONFLICT (request_id) DO UPDATE SET
                route = EXCLUDED.route,
                provider = EXCLUDED.provider,
                service_name = EXCLUDED.service_name,
                policy_trace = EXCLUDED.policy_trace,
                guardrail_trace = EXCLUDED.guardrail_trace,
                selection_trace = EXCLUDED.selection_trace,
                fallback_history = EXCLUDED.fallback_history,
                upstream_latency_ms = EXCLUDED.upstream_latency_ms,
                request_hash = EXCLUDED.request_hash,
                response_hash = EXCLUDED.response_hash,
                redaction_version = EXCLUDED.redaction_version
            "#,
        )
        .bind(&bundle.request_id)
        .bind(bundle.route.map(route_str))
        .bind(bundle.provider.map(provider_str))
        .bind(&bundle.service_name)
        .bind(Json(&bundle.policy_trace))
        .bind(Json(&bundle.guardrail_trace))
        .bind(Json(&bundle.selection_trace))
        .bind(Json(&bundle.fallback_history))
        .bind(bundle.upstream_latency_ms)
        .bind(&bundle.request_hash)
        .bind(&bundle.response_hash)
        .bind(bundle.redaction_version)
        .bind(bundle.created_at)
        .execute(&self.pool)
        .await
        .map_err(|_| GatewayError::StoreUnavailable)?;

        Ok(())
    }

    async fn list_service_registry_snapshots(&self) -> GatewayResult<Vec<ServiceRegistrySnapshot>> {
        sqlx::query(
            r#"
            SELECT
                version,
                source,
                diff,
                services_json,
                activated_at,
                rolled_back_from_version,
                created_at
            FROM service_registry_snapshots
            ORDER BY version DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map(|rows| {
            rows.into_iter()
                .map(|row| service_registry_snapshot_from_row(&row))
                .collect::<GatewayResult<Vec<_>>>()
        })
        .map_err(|_| GatewayError::StoreUnavailable)?
    }

    async fn insert_service_registry_snapshot(
        &self,
        snapshot: ServiceRegistrySnapshot,
    ) -> GatewayResult<ServiceRegistrySnapshot> {
        let row = sqlx::query(
            r#"
            INSERT INTO service_registry_snapshots (
                source,
                diff,
                services_json,
                activated_at,
                rolled_back_from_version,
                created_at
            )
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING
                version,
                source,
                diff,
                services_json,
                activated_at,
                rolled_back_from_version,
                created_at
            "#,
        )
        .bind(&snapshot.source)
        .bind(Json(&snapshot.diff))
        .bind(Json(&snapshot.services_json))
        .bind(snapshot.activated_at)
        .bind(snapshot.rolled_back_from_version)
        .bind(snapshot.created_at)
        .fetch_one(&self.pool)
        .await
        .map_err(|_| GatewayError::StoreUnavailable)?;

        service_registry_snapshot_from_row(&row)
    }

    async fn service_registry_snapshot(
        &self,
        version: i64,
    ) -> GatewayResult<Option<ServiceRegistrySnapshot>> {
        sqlx::query(
            r#"
            SELECT
                version,
                source,
                diff,
                services_json,
                activated_at,
                rolled_back_from_version,
                created_at
            FROM service_registry_snapshots
            WHERE version = $1
            "#,
        )
        .bind(version)
        .fetch_optional(&self.pool)
        .await
        .map(|row| {
            row.map(|row| service_registry_snapshot_from_row(&row))
                .transpose()
        })
        .map_err(|_| GatewayError::StoreUnavailable)?
    }
}

fn apply_policy_patch(
    mut policy: KeyPolicy,
    patch: gateway_core::admin::KeyPolicyPatch,
) -> GatewayResult<KeyPolicy> {
    if let Some(deny) = patch.deny {
        policy.deny = deny;
    }
    if let Some(allowed_routes) = patch.allowed_routes {
        policy.allowed_routes = parse_routes(&allowed_routes)?;
    }
    if let Some(allowed_models) = patch.allowed_models {
        policy.allowed_models = allowed_models;
    }
    if let Some(allowed_providers) = patch.allowed_providers {
        policy.allowed_providers = parse_providers(&allowed_providers)?;
    }
    if let Some(allowed_services) = patch.allowed_services {
        policy.allowed_services = allowed_services;
    }
    if let Some(rpm_limit) = patch.rpm_limit {
        policy.rpm_limit = rpm_limit;
    }
    if let Some(tpm_limit) = patch.tpm_limit {
        policy.tpm_limit = tpm_limit;
    }
    if let Some(daily_budget_usd) = patch.daily_budget_usd {
        policy.daily_budget_usd = daily_budget_usd;
    }
    if let Some(monthly_budget_usd) = patch.monthly_budget_usd {
        policy.monthly_budget_usd = monthly_budget_usd;
    }
    if let Some(allow_streaming) = patch.allow_streaming {
        policy.allow_streaming = allow_streaming;
    }
    if let Some(allow_tools) = patch.allow_tools {
        policy.allow_tools = allow_tools;
    }
    if let Some(max_requests_per_day) = patch.max_requests_per_day {
        policy.max_requests_per_day = max_requests_per_day;
    }
    if let Some(max_tokens_per_day) = patch.max_tokens_per_day {
        policy.max_tokens_per_day = max_tokens_per_day;
    }
    if let Some(max_cost_per_request) = patch.max_cost_per_request {
        policy.max_cost_per_request = max_cost_per_request;
    }
    if let Some(max_input_tokens_per_request) = patch.max_input_tokens_per_request {
        policy.max_input_tokens_per_request = max_input_tokens_per_request;
    }
    if let Some(max_output_tokens_per_request) = patch.max_output_tokens_per_request {
        policy.max_output_tokens_per_request = max_output_tokens_per_request;
    }
    if let Some(allowed_hours_utc) = patch.allowed_hours_utc {
        if allowed_hours_utc
            .iter()
            .any(|hour| !(0..=23).contains(hour))
        {
            return Err(GatewayError::PolicyDenied);
        }
        policy.allowed_hours_utc = allowed_hours_utc;
    }
    if let Some(unused_key_auto_disable_after_days) = patch.unused_key_auto_disable_after_days {
        policy.unused_key_auto_disable_after_days = unused_key_auto_disable_after_days;
    }
    if let Some(max_request_body_bytes) = patch.max_request_body_bytes {
        policy.max_request_body_bytes = max_request_body_bytes;
    }
    if let Some(max_response_body_bytes) = patch.max_response_body_bytes {
        policy.max_response_body_bytes = max_response_body_bytes;
    }
    if let Some(max_stream_duration_seconds) = patch.max_stream_duration_seconds {
        policy.max_stream_duration_seconds = max_stream_duration_seconds;
    }
    if let Some(max_sse_event_bytes) = patch.max_sse_event_bytes {
        policy.max_sse_event_bytes = max_sse_event_bytes;
    }
    if let Some(max_tool_call_count) = patch.max_tool_call_count {
        policy.max_tool_call_count = max_tool_call_count;
    }
    if let Some(max_tool_schema_bytes) = patch.max_tool_schema_bytes {
        policy.max_tool_schema_bytes = max_tool_schema_bytes;
    }
    Ok(policy)
}

fn route_strings(routes: &[Route]) -> Vec<String> {
    routes
        .iter()
        .map(|route| route.as_str().to_owned())
        .collect()
}

fn provider_strings(providers: &[Provider]) -> Vec<String> {
    providers
        .iter()
        .map(|provider| match provider {
            Provider::LiteLlm => "litellm".to_owned(),
            Provider::OpenAiCompatible => "openai-compatible".to_owned(),
            Provider::InternalService => "internal-service".to_owned(),
        })
        .collect()
}

fn mode_strings(modes: &[GuardrailMode]) -> Vec<String> {
    modes.iter().map(|mode| mode.as_str().to_owned()).collect()
}

fn parse_route_value(value: &str) -> GatewayResult<Route> {
    parse_routes(&[value.to_owned()])?
        .into_iter()
        .next()
        .ok_or(GatewayError::StoreUnavailable)
}

fn parse_provider_value(value: &str) -> GatewayResult<Provider> {
    parse_providers(&[value.to_owned()])?
        .into_iter()
        .next()
        .ok_or(GatewayError::StoreUnavailable)
}

fn key_owner_type_str(owner_type: AdminKeyOwnerType) -> &'static str {
    match owner_type {
        AdminKeyOwnerType::Project => "project",
        AdminKeyOwnerType::Individual => "individual",
    }
}

fn parse_key_owner_type(value: &str) -> GatewayResult<AdminKeyOwnerType> {
    match value {
        "project" => Ok(AdminKeyOwnerType::Project),
        "individual" => Ok(AdminKeyOwnerType::Individual),
        _ => Err(GatewayError::StoreUnavailable),
    }
}

fn parse_policy_layer_kind(value: &str) -> GatewayResult<PolicyLayerKind> {
    match value {
        "global" => Ok(PolicyLayerKind::Global),
        "project" => Ok(PolicyLayerKind::Project),
        "team" => Ok(PolicyLayerKind::Team),
        "key" => Ok(PolicyLayerKind::Key),
        "route" => Ok(PolicyLayerKind::Route),
        "model" => Ok(PolicyLayerKind::Model),
        _ => Err(GatewayError::StoreUnavailable),
    }
}

fn normalize_policy_layer_scope(
    kind: PolicyLayerKind,
    scope_id: Option<String>,
) -> GatewayResult<Option<String>> {
    let scope_id = scope_id
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    match kind {
        PolicyLayerKind::Global => Ok(None),
        PolicyLayerKind::Project
        | PolicyLayerKind::Team
        | PolicyLayerKind::Key
        | PolicyLayerKind::Route
        | PolicyLayerKind::Model => scope_id.map(Some).ok_or(GatewayError::PolicyDenied),
    }
}

fn validate_key_owner(
    owner_type: AdminKeyOwnerType,
    project_id: Option<Uuid>,
) -> GatewayResult<()> {
    match (owner_type, project_id) {
        (AdminKeyOwnerType::Project, Some(_)) => Ok(()),
        (AdminKeyOwnerType::Individual, None) => Ok(()),
        _ => Err(GatewayError::InvalidProjectPayload),
    }
}

fn service_route_policy_route(route_pattern: &str) -> &'static str {
    match route_pattern {
        "/summary" => "/summary",
        "/translation" => "/translation",
        "/ocr" => "/ocr",
        "/embeddings" => "/embeddings",
        _ => "/services/*",
    }
}

fn parse_routes(values: &[String]) -> GatewayResult<Vec<Route>> {
    values
        .iter()
        .map(|value| match value.as_str() {
            "/v1/chat/completions" => Ok(Route::ChatCompletions),
            "/v1/responses" => Ok(Route::Responses),
            "/providers/openai/*" => Ok(Route::DirectOpenAi),
            "/summary" => Ok(Route::Summary),
            "/translation" => Ok(Route::Translation),
            "/ocr" => Ok(Route::Ocr),
            "/embeddings" => Ok(Route::Embeddings),
            "/services/*" => Ok(Route::ServiceWildcard),
            _ => Err(GatewayError::PolicyDenied),
        })
        .collect()
}

fn parse_providers(values: &[String]) -> GatewayResult<Vec<Provider>> {
    values
        .iter()
        .map(|value| match value.as_str() {
            "litellm" => Ok(Provider::LiteLlm),
            "openai-compatible" => Ok(Provider::OpenAiCompatible),
            "internal-service" => Ok(Provider::InternalService),
            _ => Err(GatewayError::PolicyDenied),
        })
        .collect()
}

fn parse_guardrail_modes(values: &[String]) -> GatewayResult<Vec<GuardrailMode>> {
    values.iter().map(|value| value.parse()).collect()
}

fn guardrail_definition_from_row(
    row: &sqlx::postgres::PgRow,
) -> GatewayResult<GuardrailDefinition> {
    let modes: Vec<String> = row
        .try_get("modes")
        .map_err(|_| GatewayError::StoreUnavailable)?;
    let failure_policy: String = row
        .try_get("failure_policy")
        .map_err(|_| GatewayError::StoreUnavailable)?;
    let config_schema: Json<serde_json::Value> = row
        .try_get("config_schema")
        .map_err(|_| GatewayError::StoreUnavailable)?;
    let config: Json<serde_json::Value> = row
        .try_get("config")
        .map_err(|_| GatewayError::StoreUnavailable)?;

    Ok(GuardrailDefinition {
        name: row
            .try_get("name")
            .map_err(|_| GatewayError::StoreUnavailable)?,
        description: row
            .try_get("description")
            .map_err(|_| GatewayError::StoreUnavailable)?,
        modes: parse_guardrail_modes(&modes)?,
        default_on: row
            .try_get("default_on")
            .map_err(|_| GatewayError::StoreUnavailable)?,
        failure_policy: failure_policy.parse()?,
        config_schema: config_schema.0,
        config: config.0,
        enabled: row
            .try_get("enabled")
            .map_err(|_| GatewayError::StoreUnavailable)?,
    })
}

fn admin_guardrail_definition_from_row(
    row: &sqlx::postgres::PgRow,
) -> GatewayResult<AdminGuardrailDefinitionResponse> {
    let definition = guardrail_definition_from_row(row)?;
    let provider_kind = if definition
        .config
        .get("provider_kind")
        .and_then(serde_json::Value::as_str)
        == Some("http")
    {
        GuardrailProviderKind::Http
    } else {
        GuardrailProviderKind::BuiltIn
    };
    Ok(AdminGuardrailDefinitionResponse {
        name: definition.name,
        description: definition.description,
        runtime_config: runtime_config_for_admin(&definition.config, &provider_kind),
        provider_kind,
        modes: definition.modes,
        default_on: definition.default_on,
        failure_policy: definition.failure_policy,
        config_schema: definition.config_schema,
        enabled: definition.enabled,
        endpoint_configured: definition
            .config
            .get("endpoint_url")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|value| !value.is_empty()),
        endpoint_url: definition
            .config
            .get("endpoint_url")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_owned),
        timeout_ms: definition.config.get("timeout_ms").and_then(|value| {
            value
                .as_u64()
                .or_else(|| value.as_i64().and_then(|value| u64::try_from(value).ok()))
        }),
        token_configured: definition
            .config
            .get("bearer_token_secret")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|value| !value.is_empty()),
    })
}

fn runtime_config_for_admin(
    config: &serde_json::Value,
    provider_kind: &GuardrailProviderKind,
) -> serde_json::Value {
    match provider_kind {
        GuardrailProviderKind::BuiltIn => config.clone(),
        GuardrailProviderKind::Http => config
            .get("provider_config")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({})),
    }
}

fn ensure_json_object(value: &serde_json::Value) -> GatewayResult<()> {
    if value.is_object() {
        Ok(())
    } else {
        Err(GatewayError::InvalidGuardrailRequest)
    }
}

fn guardrail_execution_event_from_row(
    row: &sqlx::postgres::PgRow,
) -> GatewayResult<GuardrailExecutionEvent> {
    let mode: String = row
        .try_get("mode")
        .map_err(|_| GatewayError::StoreUnavailable)?;
    let action: String = row
        .try_get("action")
        .map_err(|_| GatewayError::StoreUnavailable)?;
    let failure_policy: String = row
        .try_get("failure_policy")
        .map_err(|_| GatewayError::StoreUnavailable)?;
    let metadata: Json<serde_json::Value> = row
        .try_get("metadata")
        .map_err(|_| GatewayError::StoreUnavailable)?;
    let route: Option<String> = row.try_get("route").ok().flatten();
    let provider: Option<String> = row.try_get("provider").ok().flatten();
    Ok(GuardrailExecutionEvent {
        request_id: row
            .try_get("request_id")
            .map_err(|_| GatewayError::StoreUnavailable)?,
        key_id: row.try_get("key_id").ok().flatten(),
        project_id: row.try_get("project_id").ok().flatten(),
        route: route.as_deref().map(parse_route_value).transpose()?,
        model: row.try_get("model").ok().flatten(),
        provider: provider.as_deref().map(parse_provider_value).transpose()?,
        guardrail_name: row
            .try_get("guardrail_name")
            .map_err(|_| GatewayError::StoreUnavailable)?,
        mode: mode.parse()?,
        action: action.parse()?,
        failure_policy: failure_policy.parse()?,
        latency_ms: row
            .try_get("latency_ms")
            .map_err(|_| GatewayError::StoreUnavailable)?,
        reason: row.try_get("reason").ok().flatten(),
        metadata: metadata.0,
        created_at: row
            .try_get("created_at")
            .map_err(|_| GatewayError::StoreUnavailable)?,
    })
}

fn guardrail_summary_from_row(
    row: &sqlx::postgres::PgRow,
) -> GatewayResult<GuardrailExecutionSummary> {
    let mode: String = row
        .try_get("mode")
        .map_err(|_| GatewayError::StoreUnavailable)?;
    let action: String = row
        .try_get("action")
        .map_err(|_| GatewayError::StoreUnavailable)?;
    let failure_policy: String = row
        .try_get("failure_policy")
        .map_err(|_| GatewayError::StoreUnavailable)?;
    Ok(GuardrailExecutionSummary {
        guardrail_name: row
            .try_get("guardrail_name")
            .map_err(|_| GatewayError::StoreUnavailable)?,
        mode: mode.parse()?,
        action: action.parse()?,
        failure_policy: failure_policy.parse()?,
        count: row
            .try_get("count")
            .map_err(|_| GatewayError::StoreUnavailable)?,
        total_latency_ms: row
            .try_get("total_latency_ms")
            .map_err(|_| GatewayError::StoreUnavailable)?,
    })
}

fn guardrail_policy_from_row(row: &sqlx::postgres::PgRow) -> GatewayResult<GuardrailPolicy> {
    let policy = GuardrailPolicy {
        mandatory_guardrails: row.try_get("mandatory_guardrails").unwrap_or_default(),
        optional_guardrails: row.try_get("optional_guardrails").unwrap_or_default(),
        forbidden_guardrails: row.try_get("forbidden_guardrails").unwrap_or_default(),
        guardrail_config_overrides: guardrail_config_overrides_from_row(row)?,
    };
    policy.validate()?;
    Ok(policy)
}

fn admin_policy_response_from_policy(policy: &KeyPolicy) -> AdminPolicyResponse {
    AdminPolicyResponse {
        deny: policy.deny,
        allowed_routes: route_strings(&policy.allowed_routes),
        allowed_models: policy.allowed_models.clone(),
        allowed_providers: provider_strings(&policy.allowed_providers),
        allowed_services: policy.allowed_services.clone(),
        rpm_limit: policy.rpm_limit,
        tpm_limit: policy.tpm_limit,
        daily_budget_usd: policy.daily_budget_usd,
        monthly_budget_usd: policy.monthly_budget_usd,
        allow_streaming: policy.allow_streaming,
        allow_tools: policy.allow_tools,
        max_requests_per_day: policy.max_requests_per_day,
        max_tokens_per_day: policy.max_tokens_per_day,
        max_cost_per_request: policy.max_cost_per_request,
        max_input_tokens_per_request: policy.max_input_tokens_per_request,
        max_output_tokens_per_request: policy.max_output_tokens_per_request,
        allowed_hours_utc: policy.allowed_hours_utc.clone(),
        unused_key_auto_disable_after_days: policy.unused_key_auto_disable_after_days,
        max_request_body_bytes: policy.max_request_body_bytes,
        max_response_body_bytes: policy.max_response_body_bytes,
        max_stream_duration_seconds: policy.max_stream_duration_seconds,
        max_sse_event_bytes: policy.max_sse_event_bytes,
        max_tool_call_count: policy.max_tool_call_count,
        max_tool_schema_bytes: policy.max_tool_schema_bytes,
        policy_version: policy.policy_version,
    }
}

fn policy_layer_response_from_row(
    row: &sqlx::postgres::PgRow,
) -> GatewayResult<AdminPolicyLayerResponse> {
    let kind: String = row
        .try_get("layer_kind")
        .map_err(|_| GatewayError::StoreUnavailable)?;
    let policy: Json<KeyPolicy> = row
        .try_get("policy")
        .map_err(|_| GatewayError::StoreUnavailable)?;
    let guardrail_policy: Json<GuardrailPolicy> = row
        .try_get("guardrail_policy")
        .map_err(|_| GatewayError::StoreUnavailable)?;
    Ok(AdminPolicyLayerResponse {
        id: row
            .try_get("id")
            .map_err(|_| GatewayError::StoreUnavailable)?,
        kind: parse_policy_layer_kind(&kind)?,
        scope_id: row.try_get("scope_id").ok().flatten(),
        policy: admin_policy_response_from_policy(&policy.0),
        guardrail_policy: guardrail_policy.0,
        created_at: row
            .try_get("created_at")
            .map_err(|_| GatewayError::StoreUnavailable)?,
        updated_at: row
            .try_get("updated_at")
            .map_err(|_| GatewayError::StoreUnavailable)?,
    })
}

fn guardrail_config_overrides_from_row(
    row: &sqlx::postgres::PgRow,
) -> GatewayResult<BTreeMap<String, serde_json::Value>> {
    let value: Json<serde_json::Value> = row
        .try_get("guardrail_config_overrides")
        .unwrap_or_else(|_| Json(serde_json::json!({})));
    value
        .0
        .as_object()
        .map(|object| {
            object
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect()
        })
        .ok_or(GatewayError::InvalidGuardrailRequest)
}

fn openai_route_setting_from_row(row: &sqlx::postgres::PgRow) -> GatewayResult<OpenAiRouteSetting> {
    Ok(OpenAiRouteSetting {
        route_id: row
            .try_get("route_id")
            .map_err(|_| GatewayError::StoreUnavailable)?,
        route: row
            .try_get("route")
            .map_err(|_| GatewayError::StoreUnavailable)?,
        enabled: row
            .try_get("enabled")
            .map_err(|_| GatewayError::StoreUnavailable)?,
        updated_at: row
            .try_get("updated_at")
            .map_err(|_| GatewayError::StoreUnavailable)?,
    })
}

fn project_response_from_row(row: sqlx::postgres::PgRow) -> GatewayResult<ProjectResponse> {
    Ok(ProjectResponse {
        id: row
            .try_get("id")
            .map_err(|_| GatewayError::StoreUnavailable)?,
        name: row
            .try_get("name")
            .map_err(|_| GatewayError::StoreUnavailable)?,
        service_names: row.try_get("service_names").unwrap_or_default(),
        created_at: row
            .try_get("created_at")
            .map_err(|_| GatewayError::StoreUnavailable)?,
        updated_at: row
            .try_get("updated_at")
            .map_err(|_| GatewayError::StoreUnavailable)?,
    })
}

fn provider_config_response_from_row(
    row: &sqlx::postgres::PgRow,
) -> GatewayResult<ProviderConfigResponse> {
    let provider: String = row
        .try_get("provider")
        .map_err(|_| GatewayError::StoreUnavailable)?;
    let credential_secret: Option<String> = row
        .try_get("credential_secret")
        .map_err(|_| GatewayError::StoreUnavailable)?;
    Ok(ProviderConfigResponse {
        id: row
            .try_get("id")
            .map_err(|_| GatewayError::StoreUnavailable)?,
        provider: parse_provider_config_kind(&provider)?,
        name: row
            .try_get("name")
            .map_err(|_| GatewayError::StoreUnavailable)?,
        base_url: row
            .try_get("base_url")
            .map_err(|_| GatewayError::StoreUnavailable)?,
        enabled: row
            .try_get("enabled")
            .map_err(|_| GatewayError::StoreUnavailable)?,
        credential_configured: credential_secret
            .as_deref()
            .is_some_and(|value| !value.is_empty()),
        created_at: row
            .try_get("created_at")
            .map_err(|_| GatewayError::StoreUnavailable)?,
        updated_at: row
            .try_get("updated_at")
            .map_err(|_| GatewayError::StoreUnavailable)?,
    })
}

fn studio_connection_from_row(
    row: &sqlx::postgres::PgRow,
) -> GatewayResult<StoredStudioConnection> {
    Ok(StoredStudioConnection {
        base_url: row
            .try_get("base_url")
            .map_err(|_| GatewayError::StoreUnavailable)?,
        bearer_token_secret: row
            .try_get("bearer_token_secret")
            .map_err(|_| GatewayError::StoreUnavailable)?,
        updated_at: row
            .try_get("updated_at")
            .map_err(|_| GatewayError::StoreUnavailable)?,
    })
}

fn admin_key_response_from_row(row: &sqlx::postgres::PgRow) -> GatewayResult<AdminKeyResponse> {
    let owner_type: String = row
        .try_get("owner_type")
        .map_err(|_| GatewayError::StoreUnavailable)?;
    Ok(AdminKeyResponse {
        id: row
            .try_get("id")
            .map_err(|_| GatewayError::StoreUnavailable)?,
        owner_type: parse_key_owner_type(&owner_type)?,
        project_id: row
            .try_get("project_id")
            .map_err(|_| GatewayError::StoreUnavailable)?,
        service_names: row.try_get("service_names").unwrap_or_default(),
        key_prefix: row
            .try_get("key_prefix")
            .map_err(|_| GatewayError::StoreUnavailable)?,
        disabled: row
            .try_get("disabled")
            .map_err(|_| GatewayError::StoreUnavailable)?,
        revoked_at: row
            .try_get("revoked_at")
            .map_err(|_| GatewayError::StoreUnavailable)?,
        expires_at: row
            .try_get("expires_at")
            .map_err(|_| GatewayError::StoreUnavailable)?,
        rotation_due_at: row.try_get("rotation_due_at").ok().flatten(),
        last_used_at: row.try_get("last_used_at").ok().flatten(),
        policy: AdminPolicyResponse {
            deny: row.try_get("deny").unwrap_or(false),
            allowed_routes: row
                .try_get("allowed_routes")
                .unwrap_or_else(|_| route_strings(&KeyPolicy::default().allowed_routes)),
            allowed_models: row.try_get("allowed_models").unwrap_or_default(),
            allowed_providers: row
                .try_get("allowed_providers")
                .unwrap_or_else(|_| provider_strings(&KeyPolicy::default().allowed_providers)),
            allowed_services: row.try_get("allowed_services").unwrap_or_default(),
            rpm_limit: row.try_get("rpm_limit").ok(),
            tpm_limit: row.try_get("tpm_limit").ok(),
            daily_budget_usd: row.try_get("daily_budget_usd").ok(),
            monthly_budget_usd: row.try_get("monthly_budget_usd").ok(),
            allow_streaming: row.try_get("allow_streaming").unwrap_or(false),
            allow_tools: row.try_get("allow_tools").unwrap_or(false),
            max_requests_per_day: row.try_get("max_requests_per_day").ok().flatten(),
            max_tokens_per_day: row.try_get("max_tokens_per_day").ok().flatten(),
            max_cost_per_request: row.try_get("max_cost_per_request").ok().flatten(),
            max_input_tokens_per_request: row
                .try_get("max_input_tokens_per_request")
                .ok()
                .flatten(),
            max_output_tokens_per_request: row
                .try_get("max_output_tokens_per_request")
                .ok()
                .flatten(),
            allowed_hours_utc: row.try_get("allowed_hours_utc").unwrap_or_default(),
            unused_key_auto_disable_after_days: row
                .try_get("unused_key_auto_disable_after_days")
                .ok()
                .flatten(),
            max_request_body_bytes: row.try_get("max_request_body_bytes").ok().flatten(),
            max_response_body_bytes: row.try_get("max_response_body_bytes").ok().flatten(),
            max_stream_duration_seconds: row.try_get("max_stream_duration_seconds").ok().flatten(),
            max_sse_event_bytes: row.try_get("max_sse_event_bytes").ok().flatten(),
            max_tool_call_count: row.try_get("max_tool_call_count").ok().flatten(),
            max_tool_schema_bytes: row.try_get("max_tool_schema_bytes").ok().flatten(),
            policy_version: row.try_get("policy_version").unwrap_or(1),
        },
        guardrail_policy: GuardrailPolicy {
            mandatory_guardrails: row.try_get("mandatory_guardrails").unwrap_or_default(),
            optional_guardrails: row.try_get("optional_guardrails").unwrap_or_default(),
            forbidden_guardrails: row.try_get("forbidden_guardrails").unwrap_or_default(),
            guardrail_config_overrides: guardrail_config_overrides_from_row(row)?,
        },
        created_at: row
            .try_get("created_at")
            .map_err(|_| GatewayError::StoreUnavailable)?,
        updated_at: row
            .try_get("updated_at")
            .map_err(|_| GatewayError::StoreUnavailable)?,
    })
}

fn operator_token_response_from_row(row: sqlx::postgres::PgRow) -> OperatorTokenResponse {
    OperatorTokenResponse {
        id: row.try_get("id").expect("operator token id"),
        token_prefix: row.try_get("token_prefix").expect("operator token prefix"),
        roles: row.try_get("roles").expect("operator token roles"),
        scopes: row.try_get("scopes").expect("operator token scopes"),
        disabled: row.try_get("disabled").expect("operator token disabled"),
        revoked_at: row
            .try_get("revoked_at")
            .expect("operator token revoked_at"),
        last_used_at: row
            .try_get("last_used_at")
            .expect("operator token last_used_at"),
        created_at: row
            .try_get("created_at")
            .expect("operator token created_at"),
        updated_at: row
            .try_get("updated_at")
            .expect("operator token updated_at"),
    }
}

fn stored_operator_token_from_row(
    row: &sqlx::postgres::PgRow,
) -> Result<StoredOperatorToken, sqlx::Error> {
    Ok(StoredOperatorToken {
        id: row.try_get("id")?,
        token_prefix: row.try_get("token_prefix")?,
        token_hash: row.try_get("token_hash")?,
        roles: row.try_get("roles")?,
        scopes: row.try_get("scopes")?,
        disabled: row.try_get("disabled")?,
        revoked_at: row.try_get("revoked_at")?,
    })
}

fn audit_event_from_row(row: sqlx::postgres::PgRow) -> Result<AuditEvent, sqlx::Error> {
    let before: Option<Json<serde_json::Value>> = row.try_get("before_json")?;
    let after: Option<Json<serde_json::Value>> = row.try_get("after_json")?;
    Ok(AuditEvent {
        id: row.try_get("id")?,
        actor_token_id: row.try_get("actor_token_id")?,
        action: row.try_get("action")?,
        target_type: row.try_get("target_type")?,
        target_id: row.try_get("target_id")?,
        before: before.map(|value| value.0),
        after: after.map(|value| value.0),
        request_id: row.try_get("request_id")?,
        ip: row.try_get("ip")?,
        user_agent: row.try_get("user_agent")?,
        created_at: row.try_get("created_at")?,
    })
}

fn service_registration_from_row(
    row: &sqlx::postgres::PgRow,
) -> Result<ServiceRegistration, sqlx::Error> {
    let cost_mode: String = row.try_get("cost_mode")?;
    let source: String = row.try_get("source")?;
    let sync_status: String = row.try_get("sync_status")?;
    Ok(ServiceRegistration {
        name: row.try_get("name")?,
        project_id: row.try_get("project_id")?,
        studio_service_id: row.try_get("studio_service_id")?,
        route_pattern: row.try_get("route_pattern")?,
        upstream_base_url: row.try_get("upstream_base_url")?,
        enabled: row.try_get("enabled")?,
        allowed_methods: row.try_get("allowed_methods")?,
        timeout_ms: row.try_get("timeout_ms")?,
        max_body_bytes: row.try_get("max_body_bytes")?,
        cost_mode: parse_service_cost_mode(&cost_mode).map_err(sqlx::Error::Decode)?,
        estimated_cost_usd: row.try_get("estimated_cost_usd")?,
        credential_secret: row.try_get("credential_secret")?,
        fallback_services: row.try_get("fallback_services")?,
        source: parse_service_source(&source).map_err(sqlx::Error::Decode)?,
        sync_status: parse_service_sync_status(&sync_status).map_err(sqlx::Error::Decode)?,
        last_synced_at: row.try_get("last_synced_at")?,
        disabled_at: row.try_get("disabled_at")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn referenced_guardrail_policy_names(policy: &GuardrailPolicy) -> Vec<String> {
    policy
        .mandatory_guardrails
        .iter()
        .chain(policy.optional_guardrails.iter())
        .chain(policy.forbidden_guardrails.iter())
        .chain(policy.guardrail_config_overrides.keys())
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn route_pattern_matches(route_pattern: &str, path: &str) -> bool {
    if let Some(prefix) = route_pattern.strip_suffix("/*") {
        path == prefix
            || path
                .strip_prefix(prefix)
                .is_some_and(|suffix| suffix.starts_with('/'))
    } else {
        path == route_pattern
    }
}

fn service_cost_mode_str(cost_mode: ServiceCostMode) -> &'static str {
    match cost_mode {
        ServiceCostMode::Fixed => "fixed",
        ServiceCostMode::Passthrough => "passthrough",
        ServiceCostMode::None => "none",
    }
}

fn parse_service_cost_mode(
    value: &str,
) -> Result<ServiceCostMode, Box<dyn std::error::Error + Send + Sync>> {
    match value {
        "fixed" => Ok(ServiceCostMode::Fixed),
        "passthrough" => Ok(ServiceCostMode::Passthrough),
        "none" => Ok(ServiceCostMode::None),
        _ => Err("invalid service cost mode".into()),
    }
}

fn service_source_str(source: ServiceSource) -> &'static str {
    match source {
        ServiceSource::Gateway => "gateway",
        ServiceSource::Studio => "studio",
    }
}

fn parse_service_source(
    value: &str,
) -> Result<ServiceSource, Box<dyn std::error::Error + Send + Sync>> {
    match value {
        "gateway" => Ok(ServiceSource::Gateway),
        "studio" => Ok(ServiceSource::Studio),
        _ => Err("invalid service source".into()),
    }
}

fn service_sync_status_str(sync_status: ServiceSyncStatus) -> &'static str {
    match sync_status {
        ServiceSyncStatus::Local => "local",
        ServiceSyncStatus::Synced => "synced",
        ServiceSyncStatus::Incomplete => "incomplete",
        ServiceSyncStatus::Stale => "stale",
        ServiceSyncStatus::Failed => "failed",
    }
}

fn parse_service_sync_status(
    value: &str,
) -> Result<ServiceSyncStatus, Box<dyn std::error::Error + Send + Sync>> {
    match value {
        "local" => Ok(ServiceSyncStatus::Local),
        "synced" => Ok(ServiceSyncStatus::Synced),
        "incomplete" => Ok(ServiceSyncStatus::Incomplete),
        "stale" => Ok(ServiceSyncStatus::Stale),
        "failed" => Ok(ServiceSyncStatus::Failed),
        _ => Err("invalid service sync status".into()),
    }
}

fn service_sync_status_for_runtime(
    upstream_base_url: Option<&str>,
    credential: Option<&str>,
    complete_status: ServiceSyncStatus,
) -> ServiceSyncStatus {
    if upstream_base_url.is_some_and(|value| !value.is_empty())
        && credential.is_some_and(|value| !value.is_empty())
    {
        complete_status
    } else {
        ServiceSyncStatus::Incomplete
    }
}

fn is_unique_violation(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .is_some_and(|database_error| database_error.code().as_deref() == Some("23505"))
}

fn is_foreign_key_violation(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .is_some_and(|database_error| database_error.code().as_deref() == Some("23503"))
}

fn is_unique_violation_on(error: &sqlx::Error, constraint: &str) -> bool {
    error.as_database_error().is_some_and(|database_error| {
        database_error.code().as_deref() == Some("23505")
            && database_error.constraint() == Some(constraint)
    })
}

fn append_usage_filters<'a>(builder: &mut QueryBuilder<'a, Postgres>, query: &'a UsageQuery) {
    append_usage_filters_with_alias(builder, query, "");
}

fn append_usage_filters_with_alias<'a>(
    builder: &mut QueryBuilder<'a, Postgres>,
    query: &'a UsageQuery,
    alias: &str,
) {
    let column = |name: &str| {
        if alias.is_empty() {
            name.to_owned()
        } else {
            format!("{alias}.{name}")
        }
    };
    let mut separated = builder.separated(" AND ");
    separated.push_unseparated(" WHERE true");
    if let Some(from) = query.from {
        separated.push(column("created_at"));
        separated.push(" >= ");
        separated.push_bind_unseparated(from);
    }
    if let Some(to) = query.to {
        separated.push(column("created_at"));
        separated.push(" < ");
        separated.push_bind_unseparated(to);
    }
    if let Some(project_id) = query.project_id {
        separated.push(column("project_id"));
        separated.push(" = ");
        separated.push_bind_unseparated(project_id);
    }
    if let Some(key_id) = query.key_id {
        separated.push(column("key_id"));
        separated.push(" = ");
        separated.push_bind_unseparated(key_id);
    }
    if let Some(route) = query.route.as_deref() {
        separated.push(column("route"));
        separated.push(" = ");
        separated.push_bind_unseparated(route);
    }
    if let Some(provider) = query.provider.as_deref() {
        separated.push(column("provider"));
        separated.push(" = ");
        separated.push_bind_unseparated(provider);
    }
    if let Some(service) = query.service.as_deref() {
        separated.push(column("service_name"));
        separated.push(" = ");
        separated.push_bind_unseparated(service);
    }
    if let Some(task_id) = query.task_id.as_deref() {
        separated.push(column("task_id"));
        separated.push(" = ");
        separated.push_bind_unseparated(task_id);
    }
    if let Some(model) = query.model.as_deref() {
        separated.push(column("model"));
        separated.push(" = ");
        separated.push_bind_unseparated(model);
    }
    if let Some(status) = query.status.as_deref() {
        separated.push(column("status"));
        separated.push(" = ");
        separated.push_bind_unseparated(status);
    }
}

fn append_guardrail_event_filters<'a>(
    builder: &mut QueryBuilder<'a, Postgres>,
    query: &'a GuardrailEventQuery,
) {
    let mut separated = builder.separated(" AND ");
    separated.push_unseparated(" WHERE true");
    if let Some(from) = query.from {
        separated.push("created_at >= ");
        separated.push_bind_unseparated(from);
    }
    if let Some(to) = query.to {
        separated.push("created_at < ");
        separated.push_bind_unseparated(to);
    }
    if let Some(project_id) = query.project_id {
        separated.push("project_id = ");
        separated.push_bind_unseparated(project_id);
    }
    if let Some(key_id) = query.key_id {
        separated.push("key_id = ");
        separated.push_bind_unseparated(key_id);
    }
    if let Some(route) = query.route.as_deref() {
        separated.push("route = ");
        separated.push_bind_unseparated(route);
    }
    if let Some(provider) = query.provider.as_deref() {
        separated.push("provider = ");
        separated.push_bind_unseparated(provider);
    }
    if let Some(model) = query.model.as_deref() {
        separated.push("model = ");
        separated.push_bind_unseparated(model);
    }
    if let Some(guardrail) = query.guardrail.as_deref() {
        separated.push("guardrail_name = ");
        separated.push_bind_unseparated(guardrail);
    }
    if let Some(mode) = query.mode.as_deref() {
        separated.push("mode = ");
        separated.push_bind_unseparated(mode);
    }
    if let Some(action) = query.action.as_deref() {
        separated.push("action = ");
        separated.push_bind_unseparated(action);
    }
}

fn summary_from_row(
    (
        request_count,
        success_count,
        failure_count,
        input_tokens,
        output_tokens,
        total_tokens,
        estimated_cost_usd,
        total_latency_ms,
        fallback_count,
    ): (i64, i64, i64, i64, i64, i64, Option<f64>, i64, i64),
) -> UsageSummary {
    UsageSummary {
        request_count,
        success_count,
        failure_count,
        input_tokens,
        output_tokens,
        total_tokens,
        estimated_cost_usd,
        total_latency_ms,
        fallback_count,
    }
}

fn provider_str(provider: Provider) -> &'static str {
    match provider {
        Provider::LiteLlm => "litellm",
        Provider::OpenAiCompatible => "openai-compatible",
        Provider::InternalService => "internal-service",
    }
}

fn route_str(route: Route) -> &'static str {
    route.as_str()
}

fn provider_health_status_str(status: ProviderHealthStatus) -> &'static str {
    match status {
        ProviderHealthStatus::Healthy => "healthy",
        ProviderHealthStatus::Degraded => "degraded",
        ProviderHealthStatus::Unhealthy => "unhealthy",
        ProviderHealthStatus::Unknown => "unknown",
    }
}

fn parse_provider_health_status(value: &str) -> GatewayResult<ProviderHealthStatus> {
    match value {
        "healthy" => Ok(ProviderHealthStatus::Healthy),
        "degraded" => Ok(ProviderHealthStatus::Degraded),
        "unhealthy" => Ok(ProviderHealthStatus::Unhealthy),
        "unknown" => Ok(ProviderHealthStatus::Unknown),
        _ => Err(GatewayError::StoreUnavailable),
    }
}

fn circuit_state_str(state: CircuitBreakerState) -> &'static str {
    match state {
        CircuitBreakerState::Closed => "closed",
        CircuitBreakerState::Open => "open",
        CircuitBreakerState::HalfOpen => "half_open",
    }
}

fn parse_circuit_state(value: &str) -> GatewayResult<CircuitBreakerState> {
    match value {
        "closed" => Ok(CircuitBreakerState::Closed),
        "open" => Ok(CircuitBreakerState::Open),
        "half_open" => Ok(CircuitBreakerState::HalfOpen),
        _ => Err(GatewayError::StoreUnavailable),
    }
}

fn provider_health_state_from_row(
    row: &sqlx::postgres::PgRow,
) -> GatewayResult<ProviderHealthState> {
    let provider: String = row
        .try_get("provider")
        .map_err(|_| GatewayError::StoreUnavailable)?;
    let status: String = row
        .try_get("status")
        .map_err(|_| GatewayError::StoreUnavailable)?;
    let circuit_state: String = row
        .try_get("circuit_state")
        .map_err(|_| GatewayError::StoreUnavailable)?;
    Ok(ProviderHealthState {
        name: row
            .try_get("name")
            .map_err(|_| GatewayError::StoreUnavailable)?,
        provider: parse_provider_value(&provider)?,
        status: parse_provider_health_status(&status)?,
        circuit_state: parse_circuit_state(&circuit_state)?,
        active_check_ok: row.try_get("active_check_ok").ok().flatten(),
        passive_success_count: row
            .try_get("passive_success_count")
            .map_err(|_| GatewayError::StoreUnavailable)?,
        passive_failure_count: row
            .try_get("passive_failure_count")
            .map_err(|_| GatewayError::StoreUnavailable)?,
        consecutive_failures: row
            .try_get("consecutive_failures")
            .map_err(|_| GatewayError::StoreUnavailable)?,
        average_latency_ms: row.try_get("average_latency_ms").ok().flatten(),
        last_error_code: row.try_get("last_error_code").ok().flatten(),
        cooldown_until: row.try_get("cooldown_until").ok().flatten(),
        checked_at: row.try_get("checked_at").ok().flatten(),
        updated_at: row
            .try_get("updated_at")
            .map_err(|_| GatewayError::StoreUnavailable)?,
    })
}

fn debug_bundle_from_row(row: &sqlx::postgres::PgRow) -> GatewayResult<DebugBundle> {
    let route: Option<String> = row.try_get("route").ok().flatten();
    let provider: Option<String> = row.try_get("provider").ok().flatten();
    let policy_trace: Json<Vec<String>> = row
        .try_get("policy_trace")
        .map_err(|_| GatewayError::StoreUnavailable)?;
    let guardrail_trace: Json<Vec<String>> = row
        .try_get("guardrail_trace")
        .map_err(|_| GatewayError::StoreUnavailable)?;
    let selection_trace: Json<Vec<String>> = row
        .try_get("selection_trace")
        .map_err(|_| GatewayError::StoreUnavailable)?;
    let fallback_history: Json<Vec<FallbackAttempt>> = row
        .try_get("fallback_history")
        .map_err(|_| GatewayError::StoreUnavailable)?;

    Ok(DebugBundle {
        request_id: row
            .try_get("request_id")
            .map_err(|_| GatewayError::StoreUnavailable)?,
        route: route.as_deref().map(parse_route_value).transpose()?,
        provider: provider.as_deref().map(parse_provider_value).transpose()?,
        service_name: row.try_get("service_name").ok().flatten(),
        policy_trace: policy_trace.0,
        guardrail_trace: guardrail_trace.0,
        selection_trace: selection_trace.0,
        fallback_history: fallback_history.0,
        upstream_latency_ms: row.try_get("upstream_latency_ms").ok().flatten(),
        request_hash: row.try_get("request_hash").ok().flatten(),
        response_hash: row.try_get("response_hash").ok().flatten(),
        redaction_version: row
            .try_get("redaction_version")
            .map_err(|_| GatewayError::StoreUnavailable)?,
        created_at: row
            .try_get("created_at")
            .map_err(|_| GatewayError::StoreUnavailable)?,
    })
}

fn service_registry_snapshot_from_row(
    row: &sqlx::postgres::PgRow,
) -> GatewayResult<ServiceRegistrySnapshot> {
    let diff: Json<ServiceImportDiff> = row
        .try_get("diff")
        .map_err(|_| GatewayError::StoreUnavailable)?;
    let services_json: Json<serde_json::Value> = row
        .try_get("services_json")
        .map_err(|_| GatewayError::StoreUnavailable)?;
    Ok(ServiceRegistrySnapshot {
        version: row
            .try_get("version")
            .map_err(|_| GatewayError::StoreUnavailable)?,
        source: row
            .try_get("source")
            .map_err(|_| GatewayError::StoreUnavailable)?,
        diff: diff.0,
        services_json: services_json.0,
        activated_at: row.try_get("activated_at").ok().flatten(),
        rolled_back_from_version: row.try_get("rolled_back_from_version").ok().flatten(),
        created_at: row
            .try_get("created_at")
            .map_err(|_| GatewayError::StoreUnavailable)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_from_row_preserves_zero_cost_aggregate() {
        let summary = summary_from_row((0, 0, 0, 0, 0, 0, Some(0.0), 0, 0));

        assert_eq!(summary.estimated_cost_usd, Some(0.0));
    }

    #[test]
    fn budget_counter_windows_use_current_utc_day_and_month() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-05-22T16:45:11Z")
            .expect("time")
            .with_timezone(&chrono::Utc);

        let (day_start, month_start) = budget_counter_windows(now).expect("windows");

        assert_eq!(day_start.to_rfc3339(), "2026-05-22T00:00:00+00:00");
        assert_eq!(month_start.to_rfc3339(), "2026-05-01T00:00:00+00:00");
    }

    #[test]
    fn persisted_service_route_patterns_match_exact_and_wildcard_paths() {
        assert!(route_pattern_matches("/summary", "/summary"));
        assert!(route_pattern_matches(
            "/services/demo/*",
            "/services/demo/run"
        ));
        assert!(route_pattern_matches("/services/demo/*", "/services/demo"));
        assert!(!route_pattern_matches("/summary", "/summary/extra"));
    }

    #[test]
    fn linked_service_routes_expand_to_policy_routes() {
        assert_eq!(service_route_policy_route("/summary"), "/summary");
        assert_eq!(service_route_policy_route("/translation"), "/translation");
        assert_eq!(service_route_policy_route("/custom/*"), "/services/*");
    }

    #[test]
    fn referenced_guardrail_policy_names_include_all_policy_fields() {
        let names = referenced_guardrail_policy_names(&GuardrailPolicy {
            mandatory_guardrails: vec!["pii-redact".to_owned(), "shared".to_owned()],
            optional_guardrails: vec!["brand".to_owned(), "shared".to_owned()],
            forbidden_guardrails: vec!["debug".to_owned()],
            guardrail_config_overrides: BTreeMap::from([
                ("brand".to_owned(), serde_json::json!({})),
                ("custom".to_owned(), serde_json::json!({})),
            ]),
        });

        assert_eq!(
            names,
            vec!["brand", "custom", "debug", "pii-redact", "shared"]
        );
    }
}
