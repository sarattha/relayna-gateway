use crate::GatewayResult;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BudgetState {
    pub daily_spend_usd: f64,
    pub monthly_spend_usd: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BudgetDecision {
    Allowed(BudgetState),
    Exceeded(BudgetState),
}

#[async_trait]
pub trait BudgetStore: Send + Sync {
    async fn check_budget(
        &self,
        key_id: Uuid,
        daily_budget_usd: Option<f64>,
        monthly_budget_usd: Option<f64>,
        now: DateTime<Utc>,
    ) -> GatewayResult<BudgetDecision>;

    async fn add_budget_spend(
        &self,
        key_id: Uuid,
        estimated_cost_usd: f64,
        now: DateTime<Utc>,
    ) -> GatewayResult<()>;
}

#[async_trait]
impl<T> BudgetStore for std::sync::Arc<T>
where
    T: BudgetStore + ?Sized,
{
    async fn check_budget(
        &self,
        key_id: Uuid,
        daily_budget_usd: Option<f64>,
        monthly_budget_usd: Option<f64>,
        now: DateTime<Utc>,
    ) -> GatewayResult<BudgetDecision> {
        (**self)
            .check_budget(key_id, daily_budget_usd, monthly_budget_usd, now)
            .await
    }

    async fn add_budget_spend(
        &self,
        key_id: Uuid,
        estimated_cost_usd: f64,
        now: DateTime<Utc>,
    ) -> GatewayResult<()> {
        (**self)
            .add_budget_spend(key_id, estimated_cost_usd, now)
            .await
    }
}

pub fn daily_budget_key(key_id: Uuid, now: DateTime<Utc>) -> String {
    format!("budget:daily:{key_id}:{}", now.format("%Y%m%d"))
}

pub fn monthly_budget_key(key_id: Uuid, now: DateTime<Utc>) -> String {
    format!("budget:monthly:{key_id}:{}", now.format("%Y%m"))
}

pub fn evaluate_budget(
    state: BudgetState,
    daily_budget_usd: Option<f64>,
    monthly_budget_usd: Option<f64>,
) -> BudgetDecision {
    let daily_exceeded = daily_budget_usd.is_some_and(|budget| state.daily_spend_usd >= budget);
    let monthly_exceeded =
        monthly_budget_usd.is_some_and(|budget| state.monthly_spend_usd >= budget);

    if daily_exceeded || monthly_exceeded {
        BudgetDecision::Exceeded(state)
    } else {
        BudgetDecision::Allowed(state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_budget_counter_keys() {
        let key_id = Uuid::parse_str("018f8d31-86a7-7c48-8f36-4d1fa4d99101").expect("uuid");
        let now = DateTime::parse_from_rfc3339("2026-05-09T14:03:02Z")
            .expect("time")
            .with_timezone(&Utc);

        assert_eq!(
            daily_budget_key(key_id, now),
            "budget:daily:018f8d31-86a7-7c48-8f36-4d1fa4d99101:20260509"
        );
        assert_eq!(
            monthly_budget_key(key_id, now),
            "budget:monthly:018f8d31-86a7-7c48-8f36-4d1fa4d99101:202605"
        );
    }

    #[test]
    fn denies_when_seeded_budget_counter_reaches_limit() {
        let state = BudgetState {
            daily_spend_usd: 10.0,
            monthly_spend_usd: 25.0,
        };

        assert_eq!(
            evaluate_budget(state, Some(10.0), None),
            BudgetDecision::Exceeded(state)
        );
        assert_eq!(
            evaluate_budget(state, Some(11.0), Some(25.0)),
            BudgetDecision::Exceeded(state)
        );
        assert_eq!(
            evaluate_budget(state, Some(11.0), Some(26.0)),
            BudgetDecision::Allowed(state)
        );
    }
}
