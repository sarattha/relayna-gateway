use chrono::{DateTime, Utc};
use gateway_core::{
    budgets::{budget_reservation_key, daily_budget_key, evaluate_budget, monthly_budget_key},
    rate_limits::{request_rate_limit_key, token_rate_limit_key},
    BudgetDecision, BudgetState, BudgetStore, GatewayError, GatewayResult, RateLimitDecision,
    RateLimitStore,
};
use redis::{aio::MultiplexedConnection, AsyncCommands};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq)]
struct StoredBudgetReservation {
    amount_usd: f64,
    daily_key: String,
    monthly_key: String,
}

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

    pub async fn seed_budget_counters(
        &self,
        key_id: Uuid,
        daily_spend_usd: f64,
        monthly_spend_usd: f64,
        now: DateTime<Utc>,
    ) -> GatewayResult<()> {
        let daily_key = daily_budget_key(key_id, now);
        let monthly_key = monthly_budget_key(key_id, now);
        let mut connection = self.connection().await?;
        let _: () = redis::pipe()
            .atomic()
            .cmd("SET")
            .arg(&daily_key)
            .arg(daily_spend_usd.max(0.0))
            .ignore()
            .cmd("EXPIRE")
            .arg(&daily_key)
            .arg(172_800)
            .ignore()
            .cmd("SET")
            .arg(&monthly_key)
            .arg(monthly_spend_usd.max(0.0))
            .ignore()
            .cmd("EXPIRE")
            .arg(&monthly_key)
            .arg(5_356_800)
            .ignore()
            .query_async(&mut connection)
            .await
            .map_err(|_| GatewayError::ControlStateUnavailable)?;
        Ok(())
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
        let (count,): (i64,) = redis::pipe()
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

    async fn check_token_rate_limit(
        &self,
        key_id: Uuid,
        tpm_limit: Option<i32>,
        estimated_tokens: i64,
        now: DateTime<Utc>,
    ) -> GatewayResult<RateLimitDecision> {
        let Some(tpm_limit) = tpm_limit else {
            return Ok(RateLimitDecision::Allowed { count: 0 });
        };
        let estimated_tokens = estimated_tokens.max(0);
        if estimated_tokens == 0 {
            return Ok(RateLimitDecision::Allowed { count: 0 });
        }

        let key = token_rate_limit_key(key_id, now);
        let mut connection = self.connection().await?;
        let (count,): (i64,) = redis::pipe()
            .atomic()
            .cmd("INCRBY")
            .arg(&key)
            .arg(estimated_tokens)
            .cmd("EXPIRE")
            .arg(&key)
            .arg(70)
            .ignore()
            .query_async(&mut connection)
            .await
            .map_err(|_| GatewayError::ControlStateUnavailable)?;

        if count > i64::from(tpm_limit) {
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

    async fn reserve_budget(
        &self,
        key_id: Uuid,
        request_id: &str,
        estimated_cost_usd: f64,
        now: DateTime<Utc>,
    ) -> GatewayResult<()> {
        if estimated_cost_usd <= 0.0 {
            return Ok(());
        }

        let reservation_key = budget_reservation_key(key_id, request_id);
        let daily_key = daily_budget_key(key_id, now);
        let monthly_key = monthly_budget_key(key_id, now);
        let mut connection = self.connection().await?;
        let _: () = redis::pipe()
            .atomic()
            .cmd("SET")
            .arg(&reservation_key)
            .arg(encode_budget_reservation(
                estimated_cost_usd,
                &daily_key,
                &monthly_key,
            ))
            .arg("EX")
            .arg(3600)
            .cmd("INCRBYFLOAT")
            .arg(&daily_key)
            .arg(estimated_cost_usd)
            .ignore()
            .cmd("EXPIRE")
            .arg(&daily_key)
            .arg(172_800)
            .ignore()
            .cmd("INCRBYFLOAT")
            .arg(&monthly_key)
            .arg(estimated_cost_usd)
            .ignore()
            .cmd("EXPIRE")
            .arg(&monthly_key)
            .arg(5_356_800)
            .ignore()
            .query_async(&mut connection)
            .await
            .map_err(|_| GatewayError::ControlStateUnavailable)?;
        Ok(())
    }

    async fn reconcile_budget_reservation(
        &self,
        key_id: Uuid,
        request_id: &str,
        actual_cost_usd: f64,
        now: DateTime<Utc>,
    ) -> GatewayResult<()> {
        let reservation_key = budget_reservation_key(key_id, request_id);
        let mut connection = self.connection().await?;
        let reservation =
            read_budget_reservation(&mut connection, &reservation_key, key_id, now).await?;
        let delta = actual_cost_usd.max(0.0) - reservation.amount_usd;
        let _: () = redis::pipe()
            .atomic()
            .cmd("INCRBYFLOAT")
            .arg(&reservation.daily_key)
            .arg(delta)
            .ignore()
            .cmd("INCRBYFLOAT")
            .arg(&reservation.monthly_key)
            .arg(delta)
            .ignore()
            .cmd("DEL")
            .arg(&reservation_key)
            .ignore()
            .query_async(&mut connection)
            .await
            .map_err(|_| GatewayError::ControlStateUnavailable)?;
        Ok(())
    }

    async fn release_budget_reservation(
        &self,
        key_id: Uuid,
        request_id: &str,
    ) -> GatewayResult<()> {
        let reservation_key = budget_reservation_key(key_id, request_id);
        let mut connection = self.connection().await?;
        let reservation =
            read_budget_reservation(&mut connection, &reservation_key, key_id, Utc::now()).await?;
        let _: () = redis::pipe()
            .atomic()
            .cmd("INCRBYFLOAT")
            .arg(&reservation.daily_key)
            .arg(-reservation.amount_usd)
            .ignore()
            .cmd("INCRBYFLOAT")
            .arg(&reservation.monthly_key)
            .arg(-reservation.amount_usd)
            .ignore()
            .cmd("DEL")
            .arg(&reservation_key)
            .ignore()
            .query_async(&mut connection)
            .await
            .map_err(|_| GatewayError::ControlStateUnavailable)?;
        Ok(())
    }
}

fn encode_budget_reservation(amount_usd: f64, daily_key: &str, monthly_key: &str) -> String {
    format!("{amount_usd}|{daily_key}|{monthly_key}")
}

fn parse_budget_reservation(
    value: &str,
    key_id: Uuid,
    fallback_now: DateTime<Utc>,
) -> Option<StoredBudgetReservation> {
    let parts: Vec<&str> = value.split('|').collect();
    if parts.len() == 3 {
        let amount_usd = parts[0].parse::<f64>().ok()?;
        if amount_usd.is_finite() && amount_usd >= 0.0 {
            return Some(StoredBudgetReservation {
                amount_usd,
                daily_key: parts[1].to_owned(),
                monthly_key: parts[2].to_owned(),
            });
        }
    }

    let amount_usd = value.parse::<f64>().ok()?;
    if !amount_usd.is_finite() || amount_usd < 0.0 {
        return None;
    }
    Some(StoredBudgetReservation {
        amount_usd,
        daily_key: daily_budget_key(key_id, fallback_now),
        monthly_key: monthly_budget_key(key_id, fallback_now),
    })
}

async fn read_budget_reservation(
    connection: &mut MultiplexedConnection,
    reservation_key: &str,
    key_id: Uuid,
    fallback_now: DateTime<Utc>,
) -> GatewayResult<StoredBudgetReservation> {
    let value: Option<String> = connection
        .get(reservation_key)
        .await
        .map_err(|_| GatewayError::ControlStateUnavailable)?;
    Ok(value
        .as_deref()
        .and_then(|value| parse_budget_reservation(value, key_id, fallback_now))
        .unwrap_or_else(|| StoredBudgetReservation {
            amount_usd: 0.0,
            daily_key: daily_budget_key(key_id, fallback_now),
            monthly_key: monthly_budget_key(key_id, fallback_now),
        }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reservation_value_preserves_original_budget_keys() {
        let key_id = Uuid::parse_str("018f8d31-86a7-7c48-8f36-4d1fa4d99101").expect("uuid");
        let now = DateTime::parse_from_rfc3339("2026-05-09T23:59:59Z")
            .expect("time")
            .with_timezone(&Utc);
        let daily_key = daily_budget_key(key_id, now);
        let monthly_key = monthly_budget_key(key_id, now);
        let encoded = encode_budget_reservation(0.25, &daily_key, &monthly_key);
        let later = DateTime::parse_from_rfc3339("2026-05-10T00:00:01Z")
            .expect("time")
            .with_timezone(&Utc);

        let parsed = parse_budget_reservation(&encoded, key_id, later).expect("reservation");

        assert_eq!(parsed.amount_usd, 0.25);
        assert_eq!(parsed.daily_key, daily_key);
        assert_eq!(parsed.monthly_key, monthly_key);
    }
}
