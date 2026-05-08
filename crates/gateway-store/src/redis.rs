use chrono::{DateTime, Utc};
use gateway_core::{
    budgets::{daily_budget_key, evaluate_budget, monthly_budget_key},
    rate_limits::request_rate_limit_key,
    BudgetDecision, BudgetState, BudgetStore, GatewayError, GatewayResult, RateLimitDecision,
    RateLimitStore,
};
use redis::{aio::MultiplexedConnection, AsyncCommands};
use uuid::Uuid;

#[derive(Clone)]
pub struct RedisReadiness {
    client: redis::Client,
}

impl RedisReadiness {
    pub fn new(redis_url: &str) -> redis::RedisResult<Self> {
        Ok(Self {
            client: redis::Client::open(redis_url)?,
        })
    }

    pub async fn ready(&self) -> redis::RedisResult<()> {
        let mut connection: MultiplexedConnection =
            self.client.get_multiplexed_async_connection().await?;
        redis::cmd("PING")
            .query_async::<String>(&mut connection)
            .await
            .map(|_| ())
    }
}

#[derive(Clone)]
pub struct RedisControlState {
    client: redis::Client,
}

impl RedisControlState {
    pub fn new(redis_url: &str) -> redis::RedisResult<Self> {
        Ok(Self {
            client: redis::Client::open(redis_url)?,
        })
    }

    async fn connection(&self) -> GatewayResult<MultiplexedConnection> {
        self.client
            .get_multiplexed_async_connection()
            .await
            .map_err(|_| GatewayError::ControlStateUnavailable)
    }

    async fn get_f64(connection: &mut MultiplexedConnection, key: &str) -> GatewayResult<f64> {
        let value: Option<String> = connection
            .get(key)
            .await
            .map_err(|_| GatewayError::ControlStateUnavailable)?;
        Ok(value
            .as_deref()
            .and_then(|value| value.parse::<f64>().ok())
            .unwrap_or(0.0))
    }
}

#[async_trait::async_trait]
impl RateLimitStore for RedisControlState {
    async fn check_request_rate_limit(
        &self,
        key_id: Uuid,
        rpm_limit: Option<i32>,
        now: DateTime<Utc>,
    ) -> GatewayResult<RateLimitDecision> {
        let Some(rpm_limit) = rpm_limit else {
            return Ok(RateLimitDecision::Allowed { count: 0 });
        };

        let key = request_rate_limit_key(key_id, now);
        let mut connection = self.connection().await?;
        let count: i64 = redis::pipe()
            .atomic()
            .cmd("INCR")
            .arg(&key)
            .cmd("EXPIRE")
            .arg(&key)
            .arg(70)
            .ignore()
            .query_async(&mut connection)
            .await
            .map_err(|_| GatewayError::ControlStateUnavailable)?;

        if count > i64::from(rpm_limit) {
            let ttl: i64 = connection
                .ttl(&key)
                .await
                .map_err(|_| GatewayError::ControlStateUnavailable)?;
            return Ok(RateLimitDecision::Exceeded {
                count,
                retry_after_seconds: u64::try_from(ttl).ok().filter(|ttl| *ttl > 0),
            });
        }

        Ok(RateLimitDecision::Allowed { count })
    }
}

#[async_trait::async_trait]
impl BudgetStore for RedisControlState {
    async fn check_budget(
        &self,
        key_id: Uuid,
        daily_budget_usd: Option<f64>,
        monthly_budget_usd: Option<f64>,
        now: DateTime<Utc>,
    ) -> GatewayResult<BudgetDecision> {
        if daily_budget_usd.is_none() && monthly_budget_usd.is_none() {
            return Ok(BudgetDecision::Allowed(BudgetState {
                daily_spend_usd: 0.0,
                monthly_spend_usd: 0.0,
            }));
        }

        let daily_key = daily_budget_key(key_id, now);
        let monthly_key = monthly_budget_key(key_id, now);
        let mut connection = self.connection().await?;
        let state = BudgetState {
            daily_spend_usd: Self::get_f64(&mut connection, &daily_key).await?,
            monthly_spend_usd: Self::get_f64(&mut connection, &monthly_key).await?,
        };

        Ok(evaluate_budget(state, daily_budget_usd, monthly_budget_usd))
    }

    async fn add_budget_spend(
        &self,
        key_id: Uuid,
        estimated_cost_usd: f64,
        now: DateTime<Utc>,
    ) -> GatewayResult<()> {
        if estimated_cost_usd <= 0.0 {
            return Ok(());
        }

        let daily_key = daily_budget_key(key_id, now);
        let monthly_key = monthly_budget_key(key_id, now);
        let mut connection = self.connection().await?;
        let _: f64 = redis::cmd("INCRBYFLOAT")
            .arg(&daily_key)
            .arg(estimated_cost_usd)
            .query_async(&mut connection)
            .await
            .map_err(|_| GatewayError::ControlStateUnavailable)?;
        let _: bool = connection
            .expire(&daily_key, 172_800)
            .await
            .map_err(|_| GatewayError::ControlStateUnavailable)?;
        let _: f64 = redis::cmd("INCRBYFLOAT")
            .arg(&monthly_key)
            .arg(estimated_cost_usd)
            .query_async(&mut connection)
            .await
            .map_err(|_| GatewayError::ControlStateUnavailable)?;
        let _: bool = connection
            .expire(&monthly_key, 5_356_800)
            .await
            .map_err(|_| GatewayError::ControlStateUnavailable)?;

        Ok(())
    }
}
