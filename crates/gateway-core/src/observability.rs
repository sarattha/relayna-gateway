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
    pub task_id: Option<String>,
    pub run_id: Option<String>,
    pub model: Option<String>,
    pub status: Option<String>,
    pub trace_id: Option<String>,
    pub min_cost_usd: Option<f64>,
    pub interval: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub breakdown_limit: Option<i64>,
    pub breakdown_offset: Option<i64>,
    pub sort_by: Option<String>,
    pub sort_order: Option<String>,
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
    pub average_latency_ms: Option<f64>,
    pub fallback_count: i64,
    pub policy_denial_count: i64,
    pub rate_limit_denial_count: i64,
    pub budget_denial_count: i64,
    pub guardrail_block_count: i64,
    pub expensive_request_count: i64,
    pub fallback_rate: f64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct UsageTimeseriesPoint {
    pub bucket: DateTime<Utc>,
    pub summary: UsageSummary,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct UsageServiceTimeseriesPoint {
    pub bucket: DateTime<Utc>,
    pub service_name: String,
    pub summary: UsageSummary,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct UsageBreakdown {
    pub name: String,
    pub summary: UsageSummary,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct UsageExportRow {
    pub request_id: String,
    pub key_id: Uuid,
    pub project_id: Option<Uuid>,
    pub route: String,
    pub model: Option<String>,
    pub provider: String,
    pub status: String,
    pub status_code: i32,
    pub latency_ms: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub total_tokens: i64,
    pub estimated_cost_usd: Option<f64>,
    pub cost_source: Option<String>,
    pub cost_mode: Option<String>,
    pub pricing_rule_name: Option<String>,
    pub service_name: Option<String>,
    pub task_id: Option<String>,
    pub run_id: Option<String>,
    pub trace_id: Option<String>,
    pub fallback_count: i32,
    pub guardrail_action_count: i64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct UsageExport {
    pub summary: UsageSummary,
    pub rows: Vec<UsageExportRow>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct UsageDashboard {
    pub summary: UsageSummary,
    pub breakdowns: UsageDashboardBreakdowns,
    pub timeseries: Vec<UsageTimeseriesPoint>,
    pub service_timeseries: Vec<UsageServiceTimeseriesPoint>,
    pub unused_keys: Vec<UnusedKey>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct UsageDashboardBreakdowns {
    pub projects: Vec<UsageBreakdown>,
    pub keys: Vec<UsageBreakdown>,
    pub services: Vec<UsageBreakdown>,
    pub providers: Vec<UsageBreakdown>,
    pub models: Vec<UsageBreakdown>,
    pub tasks: Vec<UsageBreakdown>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct UsageEventsPage {
    pub rows: Vec<UsageExportRow>,
    pub limit: i64,
    pub offset: i64,
    pub has_more: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct UsageFilterValuesQuery {
    pub field: String,
    pub q: Option<String>,
    #[serde(flatten)]
    pub usage: UsageQuery,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct UsageFilterValues {
    pub field: String,
    pub values: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct UnusedKey {
    pub key_id: Uuid,
    pub key_prefix: String,
    pub project_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
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

    async fn usage_export(&self, query: UsageQuery) -> GatewayResult<UsageExport>;

    async fn usage_dashboard(&self, query: UsageQuery) -> GatewayResult<UsageDashboard>;

    async fn usage_events(&self, query: UsageQuery) -> GatewayResult<UsageEventsPage>;

    async fn usage_filter_values(
        &self,
        query: UsageFilterValuesQuery,
    ) -> GatewayResult<UsageFilterValues>;

    async fn provider_health(&self, query: UsageQuery) -> GatewayResult<Vec<ProviderHealth>>;

    async fn unused_keys(&self, query: UsageQuery) -> GatewayResult<Vec<UnusedKey>>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageBreakdownDimension {
    Key,
    Project,
    Model,
    Provider,
    Service,
    Task,
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

    async fn usage_export(&self, query: UsageQuery) -> GatewayResult<UsageExport> {
        (**self).usage_export(query).await
    }

    async fn usage_dashboard(&self, query: UsageQuery) -> GatewayResult<UsageDashboard> {
        (**self).usage_dashboard(query).await
    }

    async fn usage_events(&self, query: UsageQuery) -> GatewayResult<UsageEventsPage> {
        (**self).usage_events(query).await
    }

    async fn usage_filter_values(
        &self,
        query: UsageFilterValuesQuery,
    ) -> GatewayResult<UsageFilterValues> {
        (**self).usage_filter_values(query).await
    }

    async fn provider_health(&self, query: UsageQuery) -> GatewayResult<Vec<ProviderHealth>> {
        (**self).provider_health(query).await
    }

    async fn unused_keys(&self, query: UsageQuery) -> GatewayResult<Vec<UnusedKey>> {
        (**self).unused_keys(query).await
    }
}
