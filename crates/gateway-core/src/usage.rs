use crate::{AuthenticatedKey, Provider, Route};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

use crate::GatewayResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageStatus {
    Success,
    Failure,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UsageEvent {
    pub request_id: String,
    pub key_id: Uuid,
    pub project_id: Uuid,
    pub route: Route,
    pub model: Option<String>,
    pub provider: Provider,
    pub status: UsageStatus,
    pub status_code: u16,
    pub latency_ms: i64,
    pub estimated_cost_usd: Option<f64>,
    pub created_at: DateTime<Utc>,
}

#[async_trait]
pub trait UsageRecorder: Send + Sync {
    async fn insert_usage_event(&self, event: &UsageEvent) -> GatewayResult<()>;
}

impl UsageEvent {
    pub fn new(
        request_id: impl Into<String>,
        key: &AuthenticatedKey,
        route: Route,
        model: Option<String>,
        status_code: u16,
        latency_ms: i64,
        created_at: DateTime<Utc>,
    ) -> Self {
        let status = if status_code < 400 {
            UsageStatus::Success
        } else {
            UsageStatus::Failure
        };

        Self {
            request_id: request_id.into(),
            key_id: key.key_id,
            project_id: key.project_id,
            route,
            model,
            provider: Provider::LiteLlm,
            status,
            status_code,
            latency_ms,
            estimated_cost_usd: None,
            created_at,
        }
    }

    pub fn with_estimated_cost_usd(mut self, estimated_cost_usd: Option<f64>) -> Self {
        self.estimated_cost_usd = estimated_cost_usd;
        self
    }
}

pub fn extract_model(body: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(body).ok()?;
    value
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(ToOwned::to_owned)
}

pub fn extract_estimated_cost_usd(body: &[u8]) -> Option<f64> {
    let value: Value = serde_json::from_slice(body).ok()?;
    let cost = [
        value.get("estimated_cost"),
        value.get("response_cost"),
        value.pointer("/usage/estimated_cost"),
        value.pointer("/usage/total_cost"),
        value.pointer("/usage/cost"),
        value.pointer("/_hidden_params/response_cost"),
    ]
    .into_iter()
    .flatten()
    .filter_map(Value::as_f64)
    .find(|cost| cost.is_finite() && *cost > 0.0);
    cost
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_model_from_generation_request() {
        assert_eq!(
            extract_model(br#"{"model":"gpt-4o-mini","messages":[]}"#),
            Some("gpt-4o-mini".to_owned())
        );
    }

    #[test]
    fn ignores_missing_model() {
        assert_eq!(extract_model(br#"{"input":"ping"}"#), None);
    }

    #[test]
    fn extracts_estimated_cost_from_common_upstream_shapes() {
        assert_eq!(
            extract_estimated_cost_usd(br#"{"usage":{"total_cost":0.0125}}"#),
            Some(0.0125)
        );
        assert_eq!(
            extract_estimated_cost_usd(br#"{"_hidden_params":{"response_cost":0.5}}"#),
            Some(0.5)
        );
        assert_eq!(extract_estimated_cost_usd(br#"{"usage":{"cost":0}}"#), None);
    }
}
