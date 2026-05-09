use crate::GatewayResult;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
pub struct UsageQuery {
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    pub project_id: Option<Uuid>,
    pub key_id: Option<Uuid>,
    pub route: Option<String>,
    pub provider: Option<String>,
    pub service: Option<String>,
    pub model: Option<String>,
    pub interval: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct UsageSummary {
    pub request_count: i64,
    pub success_count: i64,
    pub failure_count: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub total_tokens: i64,
    pub estimated_cost_usd: Option<f64>,
    pub total_latency_ms: i64,
    pub fallback_count: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct UsageTimeseriesPoint {
    pub bucket: DateTime<Utc>,
    pub summary: UsageSummary,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct UsageBreakdown {
    pub name: String,
    pub summary: UsageSummary,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ProviderHealth {
    pub name: String,
    pub request_count: i64,
    pub error_count: i64,
    pub timeout_count: i64,
    pub fallback_count: i64,
    pub total_latency_ms: i64,
}

#[async_trait]
pub trait UsageQueryStore: Send + Sync {
    async fn usage_summary(&self, query: UsageQuery) -> GatewayResult<UsageSummary>;

    async fn usage_timeseries(&self, query: UsageQuery)
        -> GatewayResult<Vec<UsageTimeseriesPoint>>;

    async fn usage_breakdown(
        &self,
        query: UsageQuery,
        dimension: UsageBreakdownDimension,
    ) -> GatewayResult<Vec<UsageBreakdown>>;

    async fn provider_health(&self, query: UsageQuery) -> GatewayResult<Vec<ProviderHealth>>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageBreakdownDimension {
    Key,
    Project,
    Model,
    Provider,
    Service,
}

#[async_trait]
impl<T> UsageQueryStore for std::sync::Arc<T>
where
    T: UsageQueryStore + ?Sized,
{
    async fn usage_summary(&self, query: UsageQuery) -> GatewayResult<UsageSummary> {
        (**self).usage_summary(query).await
    }

    async fn usage_timeseries(
        &self,
        query: UsageQuery,
    ) -> GatewayResult<Vec<UsageTimeseriesPoint>> {
        (**self).usage_timeseries(query).await
    }

    async fn usage_breakdown(
        &self,
        query: UsageQuery,
        dimension: UsageBreakdownDimension,
    ) -> GatewayResult<Vec<UsageBreakdown>> {
        (**self).usage_breakdown(query, dimension).await
    }

    async fn provider_health(&self, query: UsageQuery) -> GatewayResult<Vec<ProviderHealth>> {
        (**self).provider_health(query).await
    }
}
