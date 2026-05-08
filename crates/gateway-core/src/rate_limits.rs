use crate::GatewayResult;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateLimitDecision {
    Allowed {
        count: i64,
    },
    Exceeded {
        count: i64,
        retry_after_seconds: Option<u64>,
    },
}

#[async_trait]
pub trait RateLimitStore: Send + Sync {
    async fn check_request_rate_limit(
        &self,
        key_id: Uuid,
        rpm_limit: Option<i32>,
        now: DateTime<Utc>,
    ) -> GatewayResult<RateLimitDecision>;
}

#[async_trait]
impl<T> RateLimitStore for std::sync::Arc<T>
where
    T: RateLimitStore + ?Sized,
{
    async fn check_request_rate_limit(
        &self,
        key_id: Uuid,
        rpm_limit: Option<i32>,
        now: DateTime<Utc>,
    ) -> GatewayResult<RateLimitDecision> {
        (**self)
            .check_request_rate_limit(key_id, rpm_limit, now)
            .await
    }
}

pub fn request_rate_limit_key(key_id: Uuid, now: DateTime<Utc>) -> String {
    format!("rl:req:{key_id}:{}", now.format("%Y%m%d%H%M"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_rpm_counter_key() {
        let key_id = Uuid::parse_str("018f8d31-86a7-7c48-8f36-4d1fa4d99101").expect("uuid");
        let now = DateTime::parse_from_rfc3339("2026-05-09T14:03:02Z")
            .expect("time")
            .with_timezone(&Utc);

        assert_eq!(
            request_rate_limit_key(key_id, now),
            "rl:req:018f8d31-86a7-7c48-8f36-4d1fa4d99101:202605091403"
        );
    }
}
