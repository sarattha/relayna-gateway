use async_trait::async_trait;
use gateway_core::{
    auth::{StoredVirtualKey, VirtualKeyLookup},
    GatewayError, GatewayResult, Provider, Route, UsageEvent, UsageRecorder, UsageStatus,
};
use sqlx::{postgres::PgPoolOptions, PgPool};
use thiserror::Error;

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
