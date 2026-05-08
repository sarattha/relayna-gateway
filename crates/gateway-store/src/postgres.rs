use async_trait::async_trait;
use gateway_core::{
    admin::{AdminKeyCreate, AdminKeyPatch, AdminKeyResponse, AdminPolicyResponse},
    auth::{StoredVirtualKey, VirtualKeyLookup},
    AdminKeyStore, AdminKeyUsageSummary, GatewayError, GatewayResult, KeyPolicy, PolicyLookup,
    ProjectUsageSummary, Provider, Route, UsageEvent, UsageRecorder, UsageStatus,
    VirtualKeyMaterial,
};
use sqlx::{postgres::PgPoolOptions, PgPool, Row};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("postgres error: {0}")]
    Postgres(#[from] sqlx::Error),
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
                rpm_limit,
                tpm_limit,
                daily_budget_usd,
                monthly_budget_usd,
                allow_streaming,
                allow_tools
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            ON CONFLICT (key_id) DO UPDATE SET
                allowed_routes = EXCLUDED.allowed_routes,
                allowed_models = EXCLUDED.allowed_models,
                allowed_providers = EXCLUDED.allowed_providers,
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

        let response = AdminKeyResponse {
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
        };

        Ok(Some(response))
    }

    pub async fn insert_usage_event(&self, event: &UsageEvent) -> GatewayResult<()> {
        let route = match event.route {
            Route::ChatCompletions => "/v1/chat/completions",
            Route::Responses => "/v1/responses",
        };
        let provider = match event.provider {
            Provider::LiteLlm => "litellm",
        };
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
                created_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
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
            ),
        >(
            r#"
            SELECT id, project_id, key_prefix, key_hash, disabled, expires_at
            FROM api_keys
            WHERE key_prefix = $1
            "#,
        )
        .bind(prefix)
        .fetch_optional(&self.pool)
        .await
        .map(|row| {
            row.map(
                |(id, project_id, key_prefix, key_hash, disabled, expires_at)| StoredVirtualKey {
                    id,
                    project_id,
                    key_prefix,
                    key_hash,
                    disabled,
                    expires_at,
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
        .map_err(|_| GatewayError::StoreUnavailable)?;

        self.upsert_policy(key_id, &policy).await?;
        self.response_for_key(key_id)
            .await?
            .ok_or(GatewayError::StoreUnavailable)
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
            return Ok(None);
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
            "#,
        )
        .bind(key_id)
        .execute(&self.pool)
        .await
        .map_err(|_| GatewayError::StoreUnavailable)?
        .rows_affected();

        if rows == 0 {
            return Ok(None);
        }
        self.response_for_key(key_id).await
    }

    async fn disable_admin_key(&self, key_id: Uuid) -> GatewayResult<Option<AdminKeyResponse>> {
        let rows = sqlx::query(
            r#"
            UPDATE api_keys
            SET disabled = true, updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(key_id)
        .execute(&self.pool)
        .await
        .map_err(|_| GatewayError::StoreUnavailable)?
        .rows_affected();

        if rows == 0 {
            return Ok(None);
        }
        self.response_for_key(key_id).await
    }

    async fn key_usage_summary(&self, key_id: Uuid) -> GatewayResult<Option<AdminKeyUsageSummary>> {
        sqlx::query_as::<_, (i64, i64, i64, Option<i64>, Option<f64>)>(
            r#"
            SELECT
                COUNT(*)::bigint,
                COUNT(*) FILTER (WHERE status = 'success')::bigint,
                COUNT(*) FILTER (WHERE status = 'failure')::bigint,
                COALESCE(SUM(latency_ms), 0)::bigint,
                SUM(estimated_cost)::double precision
            FROM usage_events
            WHERE key_id = $1
            "#,
        )
        .bind(key_id)
        .fetch_optional(&self.pool)
        .await
        .map(|row| {
            row.map(
                |(
                    request_count,
                    success_count,
                    failure_count,
                    total_latency_ms,
                    estimated_cost_usd,
                )| {
                    AdminKeyUsageSummary {
                        key_id,
                        request_count,
                        success_count,
                        failure_count,
                        total_latency_ms: total_latency_ms.unwrap_or(0),
                        estimated_cost_usd,
                    }
                },
            )
        })
        .map_err(|_| GatewayError::StoreUnavailable)
    }

    async fn project_usage_summary(&self, project_id: Uuid) -> GatewayResult<ProjectUsageSummary> {
        sqlx::query_as::<_, (i64, i64, i64, Option<i64>, Option<f64>)>(
            r#"
            SELECT
                COUNT(*)::bigint,
                COUNT(*) FILTER (WHERE status = 'success')::bigint,
                COUNT(*) FILTER (WHERE status = 'failure')::bigint,
                COALESCE(SUM(latency_ms), 0)::bigint,
                SUM(estimated_cost)::double precision
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
                estimated_cost_usd,
            )| {
                ProjectUsageSummary {
                    project_id,
                    request_count,
                    success_count,
                    failure_count,
                    total_latency_ms: total_latency_ms.unwrap_or(0),
                    estimated_cost_usd,
                }
            },
        )
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
        })
        .collect()
}

fn parse_routes(values: &[String]) -> GatewayResult<Vec<Route>> {
    values
        .iter()
        .map(|value| match value.as_str() {
            "/v1/chat/completions" => Ok(Route::ChatCompletions),
            "/v1/responses" => Ok(Route::Responses),
            _ => Err(GatewayError::PolicyDenied),
        })
        .collect()
}

fn parse_providers(values: &[String]) -> GatewayResult<Vec<Provider>> {
    values
        .iter()
        .map(|value| match value.as_str() {
            "litellm" => Ok(Provider::LiteLlm),
            _ => Err(GatewayError::PolicyDenied),
        })
        .collect()
}
