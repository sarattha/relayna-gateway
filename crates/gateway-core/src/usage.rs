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

#[derive(Debug, Clone, PartialEq, Eq)]
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
        let status = if status_code < 500 {
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
            created_at,
        }
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
}
