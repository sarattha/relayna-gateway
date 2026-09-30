use chrono::{DateTime, Utc};
use gateway_core::{
    budgets::{budget_reservation_key, daily_budget_key, evaluate_budget, monthly_budget_key},
    rate_limits::{request_rate_limit_key, token_rate_limit_key},
    BudgetDecision, BudgetState, BudgetStore, GatewayError, GatewayResult, RateLimitDecision,
    RateLimitStore,
};
use redis::{aio::MultiplexedConnection, AsyncCommands};
use std::sync::Arc;
use uuid::Uuid;
const BUDGET_EPOCH_KEY: &str = "budget:accounting-epoch:v2";

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
    budget_epoch: Arc<std::sync::OnceLock<String>>,
}

impl RedisControlState {
    pub fn new(redis_url: &str) -> redis::RedisResult<Self> {
        Ok(Self {
            client: redis::Client::open(redis_url)?,
            budget_epoch: Arc::new(std::sync::OnceLock::new()),
        })
    }

    async fn connection(&self) -> GatewayResult<MultiplexedConnection> {
        self.client
            .get_multiplexed_async_connection()
            .await
            .map_err(|_| GatewayError::ControlStateUnavailable)
    }

    async fn budget_epoch(&self) -> GatewayResult<&str> {
        // Once armed, a running replica cannot silently restart accounting after
        // Redis loss. Recovery requires draining requests and restarting replicas.
        if let Some(epoch) = self.budget_epoch.get() {
            return Ok(epoch);
        }
        let epoch: String = redis::Script::new(
            r#"
            redis.call('SET', KEYS[1], ARGV[1], 'NX')
            return redis.call('GET', KEYS[1])
        "#,
        )
        .key(BUDGET_EPOCH_KEY)
        .arg(Uuid::new_v4().to_string())
        .invoke_async(&mut self.connection().await?)
        .await
        .map_err(|_| GatewayError::ControlStateUnavailable)?;
        let _ = self.budget_epoch.set(epoch);
        self.budget_epoch
            .get()
            .map(String::as_str)
            .ok_or(GatewayError::ControlStateUnavailable)
    }

    pub async fn seed_budget_counters(
        &self,
        key_id: Uuid,
        daily_spend_usd: f64,
        monthly_spend_usd: f64,
        now: DateTime<Utc>,
    ) -> GatewayResult<()> {
        if !daily_spend_usd.is_finite() || !monthly_spend_usd.is_finite() {
            return Err(GatewayError::ControlStateUnavailable);
        }
        let _: () = redis::Script::new(r#"
            if redis.call('GET', KEYS[3]) ~= ARGV[5] then return redis.error_reply('budget recovery required') end
            for i = 1, 2 do
                local current = tonumber(redis.call('GET', KEYS[i]) or '0')
                if not current or current < 0 then return redis.error_reply('invalid budget state') end
            end
            for i = 1, 2 do
                local current = tonumber(redis.call('GET', KEYS[i]) or '0')
                redis.call('SET', KEYS[i], math.max(current, tonumber(ARGV[i])), 'EX', ARGV[i + 2])
            end
            return nil
        "#)
        .key(daily_budget_key(key_id, now))
        .key(monthly_budget_key(key_id, now))
        .key(BUDGET_EPOCH_KEY)
        .arg(daily_spend_usd.max(0.0)).arg(monthly_spend_usd.max(0.0))
        .arg(172_800).arg(5_356_800).arg(self.budget_epoch().await?)
        .invoke_async(&mut self.connection().await?).await
        .map_err(|_| GatewayError::ControlStateUnavailable)?;
        Ok(())
    }

    async fn finalize_reservation(
        &self,
        key_id: Uuid,
        reservation_id: &str,
        actual: f64,
        now: DateTime<Utc>,
    ) -> GatewayResult<()> {
        if !actual.is_finite() || actual < 0.0 {
            return Err(GatewayError::ControlStateUnavailable);
        }
        let _: i64 = redis::Script::new(r#"
            if redis.call('GET', KEYS[4]) ~= ARGV[2] then return redis.error_reply('budget recovery required') end
            local value = redis.call('GET', KEYS[1])
            if not value then return 0 end
            local amount, daily, monthly, version = string.match(value, '^([^|]+)|([^|]+)|([^|]+)|([^|]+)$')
            amount = tonumber(amount)
            if not amount or amount < 0 or version ~= 'v2' then
                return redis.error_reply('invalid or legacy reservation; drain before upgrade')
            end
            local actual = tonumber(ARGV[1])
            local d = tonumber(redis.call('GET', daily) or '')
            local m = tonumber(redis.call('GET', monthly) or '')
            if not d or not m or d < 0 or m < 0 then return redis.error_reply('missing budget state') end
            local dr = tonumber(redis.call('GET', daily .. ':reserved') or '')
            local mr = tonumber(redis.call('GET', monthly .. ':reserved') or '')
            if not dr or not mr or dr + 0.000000001 < amount or mr + 0.000000001 < amount then
                return redis.error_reply('missing reservation state')
            end
            redis.call('SET', daily .. ':reserved', math.max(0, dr - amount), 'EX', 172800)
            redis.call('SET', monthly .. ':reserved', math.max(0, mr - amount), 'EX', 5356800)
            redis.call('INCRBYFLOAT', daily, actual)
            redis.call('INCRBYFLOAT', monthly, actual)
            redis.call('DEL', KEYS[1])
            return 1
        "#)
        .key(budget_reservation_key(key_id, reservation_id))
        .key(daily_budget_key(key_id, now)).key(monthly_budget_key(key_id, now)).key(BUDGET_EPOCH_KEY)
        .arg(actual).arg(self.budget_epoch().await?).invoke_async(&mut self.connection().await?).await
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

        let (daily, monthly): (f64, f64) = redis::Script::new(r#"
            if redis.call('GET', KEYS[3]) ~= ARGV[1] then return redis.error_reply('budget recovery required') end
            local d = tonumber(redis.call('GET', KEYS[1]) or '')
            local m = tonumber(redis.call('GET', KEYS[2]) or '')
            local dr = tonumber(redis.call('GET', KEYS[1] .. ':reserved') or '0')
            local mr = tonumber(redis.call('GET', KEYS[2] .. ':reserved') or '0')
            if not d or not m or not dr or not mr or d < 0 or m < 0 or dr < 0 or mr < 0 then
                return redis.error_reply('budget requires rehydration')
            end
            return {tostring(d + dr), tostring(m + mr)}
        "#).key(daily_budget_key(key_id, now)).key(monthly_budget_key(key_id, now))
        .key(BUDGET_EPOCH_KEY).arg(self.budget_epoch().await?)
        .invoke_async(&mut self.connection().await?).await
        .map_err(|_| GatewayError::ControlStateUnavailable)?;
        let state = BudgetState {
            daily_spend_usd: daily,
            monthly_spend_usd: monthly,
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

        if !estimated_cost_usd.is_finite() {
            return Err(GatewayError::ControlStateUnavailable);
        }
        let _: () = redis::Script::new(r#"
            if redis.call('GET', KEYS[3]) ~= ARGV[2] then return redis.error_reply('budget recovery required') end
            for i = 1, 2 do
                local current = tonumber(redis.call('GET', KEYS[i]) or '0')
                if not current or current < 0 then return redis.error_reply('invalid budget state') end
            end
            redis.call('INCRBYFLOAT', KEYS[1], ARGV[1])
            redis.call('EXPIRE', KEYS[1], 172800)
            redis.call('INCRBYFLOAT', KEYS[2], ARGV[1])
            redis.call('EXPIRE', KEYS[2], 5356800)
            return nil
        "#).key(daily_budget_key(key_id, now)).key(monthly_budget_key(key_id, now))
        .key(BUDGET_EPOCH_KEY).arg(estimated_cost_usd).arg(self.budget_epoch().await?)
        .invoke_async(&mut self.connection().await?).await
        .map_err(|_| GatewayError::ControlStateUnavailable)?;

        Ok(())
    }

    async fn seed_committed_budget(
        &self,
        key_id: Uuid,
        state: BudgetState,
        now: DateTime<Utc>,
    ) -> GatewayResult<()> {
        self.seed_budget_counters(key_id, state.daily_spend_usd, state.monthly_spend_usd, now)
            .await
    }

    async fn admit_budget(
        &self,
        key_id: Uuid,
        reservation_id: &str,
        estimated_cost_usd: f64,
        daily_budget_usd: Option<f64>,
        monthly_budget_usd: Option<f64>,
        now: DateTime<Utc>,
    ) -> GatewayResult<BudgetDecision> {
        if !estimated_cost_usd.is_finite()
            || estimated_cost_usd < 0.0
            || [daily_budget_usd, monthly_budget_usd]
                .into_iter()
                .flatten()
                .any(|v| !v.is_finite() || v < 0.0)
        {
            return Err(GatewayError::ControlStateUnavailable);
        }
        let daily = daily_budget_key(key_id, now);
        let monthly = monthly_budget_key(key_id, now);
        let (allowed, daily_spend, monthly_spend): (i64, f64, f64) = redis::Script::new(r#"
            if redis.call('GET', KEYS[4]) ~= ARGV[4] then return redis.error_reply('budget recovery required') end
            if redis.call('EXISTS', KEYS[1]) == 1 then return redis.error_reply('duplicate reservation') end
            local d = redis.call('GET', KEYS[2]); local m = redis.call('GET', KEYS[3])
            if (ARGV[2] ~= '' or ARGV[3] ~= '') and (not d or not m) then
                return redis.error_reply('budget requires rehydration')
            end
            d = tonumber(d or '0'); m = tonumber(m or '0')
            local dr = tonumber(redis.call('GET', KEYS[2] .. ':reserved') or '0')
            local mr = tonumber(redis.call('GET', KEYS[3] .. ':reserved') or '0')
            if not d or not m or not dr or not mr or d < 0 or m < 0 or dr < 0 or mr < 0 then
                return redis.error_reply('invalid budget state')
            end
            local amount = tonumber(ARGV[1]); local dl = tonumber(ARGV[2]); local ml = tonumber(ARGV[3])
            if (dl and (d + dr >= dl or d + dr + amount > dl))
                or (ml and (m + mr >= ml or m + mr + amount > ml)) then
                return {0, tostring(d + dr), tostring(m + mr)}
            end
            redis.call('SET', KEYS[1], ARGV[1] .. '|' .. KEYS[2] .. '|' .. KEYS[3] .. '|v2', 'EX', 5356800)
            redis.call('SET', KEYS[2], d, 'EX', 172800)
            redis.call('SET', KEYS[3], m, 'EX', 5356800)
            redis.call('INCRBYFLOAT', KEYS[2] .. ':reserved', amount)
            redis.call('EXPIRE', KEYS[2] .. ':reserved', 172800)
            redis.call('INCRBYFLOAT', KEYS[3] .. ':reserved', amount)
            redis.call('EXPIRE', KEYS[3] .. ':reserved', 5356800)
            return {1, tostring(d + dr + amount), tostring(m + mr + amount)}
        "#)
        .key(budget_reservation_key(key_id, reservation_id)).key(&daily).key(&monthly).key(BUDGET_EPOCH_KEY)
        .arg(estimated_cost_usd).arg(daily_budget_usd.map(|v| v.to_string()).unwrap_or_default())
        .arg(monthly_budget_usd.map(|v| v.to_string()).unwrap_or_default())
        .arg(self.budget_epoch().await?)
        .invoke_async(&mut self.connection().await?).await
        .map_err(|_| GatewayError::ControlStateUnavailable)?;
        let state = BudgetState {
            daily_spend_usd: daily_spend,
            monthly_spend_usd: monthly_spend,
        };
        Ok(if allowed == 1 {
            BudgetDecision::Allowed(state)
        } else {
            BudgetDecision::Exceeded(state)
        })
    }

    async fn reserve_budget(
        &self,
        key_id: Uuid,
        request_id: &str,
        estimated_cost_usd: f64,
        now: DateTime<Utc>,
    ) -> GatewayResult<()> {
        self.admit_budget(key_id, request_id, estimated_cost_usd, None, None, now)
            .await?;
        Ok(())
    }

    async fn reconcile_budget_reservation(
        &self,
        key_id: Uuid,
        request_id: &str,
        actual_cost_usd: f64,
        now: DateTime<Utc>,
    ) -> GatewayResult<()> {
        self.finalize_reservation(key_id, request_id, actual_cost_usd, now)
            .await
    }

    async fn release_budget_reservation(
        &self,
        key_id: Uuid,
        request_id: &str,
    ) -> GatewayResult<()> {
        self.finalize_reservation(key_id, request_id, 0.0, Utc::now())
            .await
    }
}

#[async_trait::async_trait]
impl gateway_core::accessa::AccessaStore for RedisControlState {
    async fn accessa_get(&self, key: &str) -> GatewayResult<Option<String>> {
        self.connection()
            .await?
            .get(key)
            .await
            .map_err(|_| GatewayError::ControlStateUnavailable)
    }
    async fn accessa_put(
        &self,
        key: &str,
        value: &str,
        ttl: u64,
        only_if_absent: bool,
    ) -> GatewayResult<bool> {
        let mut command = redis::cmd("SET");
        command.arg(key).arg(value).arg("EX").arg(ttl.max(1));
        if only_if_absent {
            command.arg("NX");
        }
        let result: Option<String> = command
            .query_async(&mut self.connection().await?)
            .await
            .map_err(|_| GatewayError::ControlStateUnavailable)?;
        Ok(result.is_some())
    }
    async fn accessa_delete(&self, key: &str) -> GatewayResult<()> {
        let _: usize = self
            .connection()
            .await?
            .del(key)
            .await
            .map_err(|_| GatewayError::ControlStateUnavailable)?;
        Ok(())
    }
}
