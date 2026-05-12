use async_trait::async_trait;
use gateway_core::{
    admin::{AdminKeyCreate, AdminKeyPatch, AdminKeyResponse, AdminPolicyResponse},
    auth::{StoredVirtualKey, VirtualKeyLookup},
    default_route_pattern, operator_token_prefix, parse_provider_config_kind,
    projects::{ProjectCreateRequest, ProjectPatchRequest, ProjectResponse},
    provider_config_kind_str,
    provider_configs::{
        AdminProviderConfigStore, ProviderConfigCreateRequest, ProviderConfigLookup,
        ProviderConfigPatchRequest, ProviderConfigResponse, ProviderRuntimeConfig,
    },
    services::{
        AdminServiceStore, ServiceCostMode, ServiceCreateRequest, ServicePatchRequest,
        ServiceRegistration, ServiceRegistryLookup, ServiceResponse, ServiceRouteLookup,
        ServiceSource, ServiceSyncStatus, ServiceSyncStatusResponse, StudioServiceImportRequest,
    },
    verify_stored_operator_token, AdminKeyStore, AdminKeyUsageSummary, AdminOpenAiRouteStore,
    AdminProjectStore, GatewayError, GatewayResult, KeyPolicy, OpenAiRouteSetting,
    OpenAiRouteSettingsLookup, OperatorTokenMaterial, OperatorTokenResponse, OperatorTokenStore,
    PolicyLookup, ProjectUsageSummary, Provider, ProviderHealth, Route, StoredOperatorToken,
    UsageBreakdown, UsageBreakdownDimension, UsageEvent, UsageQuery, UsageQueryStore,
    UsageRecorder, UsageStatus, UsageSummary, UsageTimeseriesPoint, VirtualKeyMaterial,
};
use sqlx::{postgres::PgPoolOptions, PgPool, Postgres, QueryBuilder, Row};
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

    pub async fn ready(&self) -> Result<(), StoreError> {
        sqlx::query("SELECT 1").execute(&self.pool).await?;
        Ok(())
    }

    async fn upsert_policy(&self, key_id: Uuid, policy: &KeyPolicy) -> GatewayResult<()> {
        sqlx::query(
            r#"
            INSERT INTO key_policies (
                key_id,
                allowed_routes,
                allowed_models,
                allowed_providers,
                allowed_services,
                rpm_limit,
                tpm_limit,
                daily_budget_usd,
                monthly_budget_usd,
                allow_streaming,
                allow_tools
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            ON CONFLICT (key_id) DO UPDATE SET
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
                updated_at = now()
            "#,
        )
        .bind(key_id)
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
        .execute(&self.pool)
        .await
        .map_err(|_| GatewayError::StoreUnavailable)?;

        Ok(())
    }

    async fn response_for_key(&self, key_id: Uuid) -> GatewayResult<Option<AdminKeyResponse>> {
        let Some(row) = sqlx::query(
            r#"
            SELECT
                k.id,
                k.project_id,
                k.key_prefix,
                k.disabled,
                k.revoked_at,
                k.expires_at,
                k.created_at,
                k.updated_at,
                p.allowed_routes,
                p.allowed_models,
                p.allowed_providers,
                p.allowed_services,
                p.rpm_limit,
                p.tpm_limit,
                p.daily_budget_usd,
                p.monthly_budget_usd,
                p.allow_streaming,
                p.allow_tools
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
            return Ok(None);
        };

        admin_key_response_from_row(&row).map(Some)
    }

    async fn stored_operator_token_by_prefix(
        &self,
        prefix: &str,
    ) -> GatewayResult<Option<StoredOperatorToken>> {
        sqlx::query(
            r#"
            SELECT id, token_prefix, token_hash, disabled, revoked_at
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
                enabled,
                cost_mode,
                estimated_cost_usd,
                source,
                sync_status,
                last_synced_at
            )
            VALUES ($1, $2, $3, $4, false, $5, $6, 'studio', 'incomplete', now())
            ON CONFLICT (studio_service_id) WHERE studio_service_id IS NOT NULL
            DO UPDATE SET
                name = EXCLUDED.name,
                project_id = EXCLUDED.project_id,
                studio_service_id = EXCLUDED.studio_service_id,
                route_pattern = EXCLUDED.route_pattern,
                cost_mode = EXCLUDED.cost_mode,
                estimated_cost_usd = EXCLUDED.estimated_cost_usd,
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

#[async_trait]
impl UsageRecorder for PostgresStore {
    async fn insert_usage_event(&self, event: &UsageEvent) -> GatewayResult<()> {
        PostgresStore::insert_usage_event(self, event).await
    }
}

#[async_trait]
impl VirtualKeyLookup for PostgresStore {
    async fn find_by_prefix(&self, prefix: &str) -> GatewayResult<Option<StoredVirtualKey>> {
        sqlx::query_as::<
            _,
            (
                uuid::Uuid,
                uuid::Uuid,
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
}

#[async_trait]
impl gateway_core::PolicyLookup for PostgresStore {
    async fn policy_for_key(&self, key_id: Uuid) -> GatewayResult<KeyPolicy> {
        let row = sqlx::query_as::<
            _,
            (
                Vec<String>,
                Vec<String>,
                Vec<String>,
                Vec<String>,
                Option<i32>,
                Option<i32>,
                Option<f64>,
                Option<f64>,
                bool,
                bool,
            ),
        >(
            r#"
            SELECT
                allowed_routes,
                allowed_models,
                allowed_providers,
                allowed_services,
                rpm_limit,
                tpm_limit,
                daily_budget_usd,
                monthly_budget_usd,
                allow_streaming,
                allow_tools
            FROM key_policies
            WHERE key_id = $1
            "#,
        )
        .bind(key_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| GatewayError::ControlStateUnavailable)?;

        let Some((
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
        )) = row
        else {
            return Ok(KeyPolicy::default());
        };

        Ok(KeyPolicy {
            allowed_routes: parse_routes(&allowed_routes)?,
            allowed_models,
            allowed_providers: parse_providers(&allowed_providers)?,
            allowed_services,
            rpm_limit,
            tpm_limit,
            daily_budget_usd,
            monthly_budget_usd,
            allow_streaming,
            allow_tools,
        })
    }
}

#[async_trait]
impl AdminProjectStore for PostgresStore {
    async fn create_project(
        &self,
        request: ProjectCreateRequest,
    ) -> GatewayResult<ProjectResponse> {
        request.validate()?;
        sqlx::query(
            r#"
            INSERT INTO projects (name)
            VALUES ($1)
            RETURNING id, name, created_at, updated_at
            "#,
        )
        .bind(request.name.trim())
        .fetch_one(&self.pool)
        .await
        .map(project_response_from_row)
        .map_err(|error| {
            if is_unique_violation(&error) {
                GatewayError::DuplicateProject
            } else {
                GatewayError::StoreUnavailable
            }
        })
    }

    async fn list_projects(&self) -> GatewayResult<Vec<ProjectResponse>> {
        sqlx::query("SELECT id, name, created_at, updated_at FROM projects ORDER BY name ASC")
            .fetch_all(&self.pool)
            .await
            .map(|rows| rows.into_iter().map(project_response_from_row).collect())
            .map_err(|_| GatewayError::StoreUnavailable)
    }

    async fn get_project(&self, project_id: Uuid) -> GatewayResult<Option<ProjectResponse>> {
        sqlx::query("SELECT id, name, created_at, updated_at FROM projects WHERE id = $1")
            .bind(project_id)
            .fetch_optional(&self.pool)
            .await
            .map(|row| row.map(project_response_from_row))
            .map_err(|_| GatewayError::StoreUnavailable)
    }

    async fn patch_project(
        &self,
        project_id: Uuid,
        patch: ProjectPatchRequest,
    ) -> GatewayResult<Option<ProjectResponse>> {
        patch.validate()?;
        let Some(name) = patch.name else {
            return self.get_project(project_id).await;
        };
        sqlx::query(
            r#"
            UPDATE projects
            SET name = $2, updated_at = now()
            WHERE id = $1
            RETURNING id, name, created_at, updated_at
            "#,
        )
        .bind(project_id)
        .bind(name.trim())
        .fetch_optional(&self.pool)
        .await
        .map(|row| row.map(project_response_from_row))
        .map_err(|error| {
            if is_unique_violation(&error) {
                GatewayError::DuplicateProject
            } else {
                GatewayError::StoreUnavailable
            }
        })
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
        let policy = apply_policy_patch(KeyPolicy::default(), request.policy)?;

        sqlx::query(
            r#"
            INSERT INTO api_keys (id, project_id, key_prefix, key_hash, expires_at)
            VALUES ($1, $2, $3, $4, $5)
            "#,
        )
        .bind(key_id)
        .bind(request.project_id)
        .bind(&material.key_prefix)
        .bind(&material.key_hash)
        .bind(request.expires_at)
        .execute(&self.pool)
        .await
        .map_err(|error| {
            if is_foreign_key_violation(&error) {
                GatewayError::MissingProject
            } else {
                GatewayError::StoreUnavailable
            }
        })?;

        self.upsert_policy(key_id, &policy).await?;
        self.response_for_key(key_id)
            .await?
            .ok_or(GatewayError::StoreUnavailable)
    }

    async fn list_admin_keys(&self) -> GatewayResult<Vec<AdminKeyResponse>> {
        let rows = sqlx::query(
            r#"
            SELECT
                k.id,
                k.project_id,
                k.key_prefix,
                k.disabled,
                k.revoked_at,
                k.expires_at,
                k.created_at,
                k.updated_at,
                p.allowed_routes,
                p.allowed_models,
                p.allowed_providers,
                p.allowed_services,
                p.rpm_limit,
                p.tpm_limit,
                p.daily_budget_usd,
                p.monthly_budget_usd,
                p.allow_streaming,
                p.allow_tools
            FROM api_keys k
            LEFT JOIN key_policies p ON p.key_id = k.id
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

        let rows = sqlx::query(
            r#"
            UPDATE api_keys
            SET
                expires_at = CASE WHEN $2 THEN $3 ELSE expires_at END,
                disabled = COALESCE($4, disabled),
                updated_at = now()
            WHERE id = $1
              AND revoked_at IS NULL
            "#,
        )
        .bind(key_id)
        .bind(update_expires_at)
        .bind(expires_at)
        .bind(patch.disabled)
        .execute(&self.pool)
        .await
        .map_err(|_| GatewayError::StoreUnavailable)?
        .rows_affected();

        if rows == 0 {
            return self.response_for_key(key_id).await;
        }

        if let Some(policy_patch) = patch.policy {
            let current = self.policy_for_key(key_id).await?;
            let policy = apply_policy_patch(current, policy_patch)?;
            self.upsert_policy(key_id, &policy).await?;
        }

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
            RETURNING id, token_prefix, disabled, revoked_at, last_used_at, created_at, updated_at
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
    ) -> GatewayResult<()> {
        let prefix = operator_token_prefix(raw_token)?;
        let stored = self
            .stored_operator_token_by_prefix(&prefix)
            .await?
            .ok_or(GatewayError::InvalidOperatorToken)?;
        verify_stored_operator_token(raw_token, &stored)?;

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
        Ok(())
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
            INSERT INTO operator_tokens (id, token_prefix, token_hash)
            VALUES ($1, $2, $3)
            RETURNING id, token_prefix, disabled, revoked_at, last_used_at, created_at, updated_at
            "#,
        )
        .bind(token_id)
        .bind(&material.token_prefix)
        .bind(&material.token_hash)
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
            UsageBreakdownDimension::Project => "project_id::text",
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

fn apply_policy_patch(
    mut policy: KeyPolicy,
    patch: gateway_core::admin::KeyPolicyPatch,
) -> GatewayResult<KeyPolicy> {
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

fn project_response_from_row(row: sqlx::postgres::PgRow) -> ProjectResponse {
    ProjectResponse {
        id: row.try_get("id").expect("project id"),
        name: row.try_get("name").expect("project name"),
        created_at: row.try_get("created_at").expect("project created_at"),
        updated_at: row.try_get("updated_at").expect("project updated_at"),
    }
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

fn admin_key_response_from_row(row: &sqlx::postgres::PgRow) -> GatewayResult<AdminKeyResponse> {
    Ok(AdminKeyResponse {
        id: row
            .try_get("id")
            .map_err(|_| GatewayError::StoreUnavailable)?,
        project_id: row
            .try_get("project_id")
            .map_err(|_| GatewayError::StoreUnavailable)?,
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
        policy: AdminPolicyResponse {
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
        disabled: row.try_get("disabled")?,
        revoked_at: row.try_get("revoked_at")?,
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
    if let Some(service) = query.service.as_deref() {
        separated.push("service_name = ");
        separated.push_bind_unseparated(service);
    }
    if let Some(task_id) = query.task_id.as_deref() {
        separated.push("task_id = ");
        separated.push_bind_unseparated(task_id);
    }
    if let Some(model) = query.model.as_deref() {
        separated.push("model = ");
        separated.push_bind_unseparated(model);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_from_row_preserves_zero_cost_aggregate() {
        let summary = summary_from_row((0, 0, 0, 0, 0, 0, Some(0.0), 0, 0));

        assert_eq!(summary.estimated_cost_usd, Some(0.0));
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
}
