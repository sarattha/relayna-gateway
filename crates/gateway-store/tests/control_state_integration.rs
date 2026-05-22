use chrono::{DateTime, Duration, Utc};
use gateway_core::{BudgetDecision, BudgetStore, RateLimitDecision, RateLimitStore};
use gateway_store::{PostgresStore, RedisControlState};
use redis::AsyncCommands;
use uuid::Uuid;

struct IntegrationEnv {
    store: PostgresStore,
    redis: RedisControlState,
    redis_client: redis::Client,
}

async fn integration_env() -> Option<IntegrationEnv> {
    let database_url = match std::env::var("DATABASE_URL") {
        Ok(value) => value,
        Err(_) => {
            eprintln!("skipping integration test: DATABASE_URL is not set");
            return None;
        }
    };
    let redis_url = match std::env::var("REDIS_URL") {
        Ok(value) => value,
        Err(_) => {
            eprintln!("skipping integration test: REDIS_URL is not set");
            return None;
        }
    };
    let store = PostgresStore::connect(&database_url)
        .await
        .expect("connect postgres");
    let redis = RedisControlState::new(&redis_url).expect("create redis control state");
    let redis_client = redis::Client::open(redis_url).expect("create redis client");
    Some(IntegrationEnv {
        store,
        redis,
        redis_client,
    })
}

async fn insert_budgeted_key(
    store: &PostgresStore,
    daily_budget_usd: Option<f64>,
    monthly_budget_usd: Option<f64>,
) -> (Uuid, Uuid) {
    let project_id = Uuid::new_v4();
    let key_id = Uuid::new_v4();
    sqlx::query("INSERT INTO projects (id, name) VALUES ($1, $2)")
        .bind(project_id)
        .bind(format!("integration-{project_id}"))
        .execute(store.pool())
        .await
        .expect("insert project");
    sqlx::query(
        r#"
        INSERT INTO api_keys (id, owner_type, project_id, key_prefix, key_hash)
        VALUES ($1, 'project', $2, $3, 'hash')
        "#,
    )
    .bind(key_id)
    .bind(project_id)
    .bind(format!("rk_live_{}", key_id.simple()))
    .execute(store.pool())
    .await
    .expect("insert key");
    sqlx::query(
        r#"
        INSERT INTO key_policies (key_id, daily_budget_usd, monthly_budget_usd)
        VALUES ($1, $2, $3)
        "#,
    )
    .bind(key_id)
    .bind(daily_budget_usd)
    .bind(monthly_budget_usd)
    .execute(store.pool())
    .await
    .expect("insert policy");
    (project_id, key_id)
}

async fn insert_usage(
    store: &PostgresStore,
    key_id: Uuid,
    project_id: Uuid,
    request_id: &str,
    estimated_cost: Option<f64>,
    created_at: DateTime<Utc>,
) {
    sqlx::query(
        r#"
        INSERT INTO usage_events (
            request_id,
            key_id,
            project_id,
            route,
            provider,
            status,
            status_code,
            latency_ms,
            estimated_cost,
            created_at
        )
        VALUES ($1, $2, $3, '/v1/chat/completions', 'litellm', 'success', 200, 10, $4, $5)
        "#,
    )
    .bind(request_id)
    .bind(key_id)
    .bind(project_id)
    .bind(estimated_cost)
    .bind(created_at)
    .execute(store.pool())
    .await
    .expect("insert usage");
}

async fn seed_from_postgres(store: &PostgresStore, redis: &RedisControlState, now: DateTime<Utc>) {
    for seed in store.budget_counter_seeds(now).await.expect("load seeds") {
        redis
            .seed_budget_counters(
                seed.key_id,
                seed.daily_spend_usd,
                seed.monthly_spend_usd,
                now,
            )
            .await
            .expect("seed redis");
    }
}

#[tokio::test]
async fn empty_redis_rehydrates_budget_spend_and_denies_over_budget_key() {
    let Some(env) = integration_env().await else {
        return;
    };
    let now = Utc::now();
    let (project_id, key_id) = insert_budgeted_key(&env.store, Some(1.0), Some(5.0)).await;
    insert_usage(
        &env.store,
        key_id,
        project_id,
        "rehydrate-over",
        Some(1.25),
        now,
    )
    .await;

    seed_from_postgres(&env.store, &env.redis, now).await;

    let decision = env
        .redis
        .check_budget(key_id, Some(1.0), Some(5.0), now)
        .await
        .expect("check budget");
    assert!(matches!(decision, BudgetDecision::Exceeded(_)));
}

#[tokio::test]
async fn rehydration_ignores_bad_costs_and_skips_unbudgeted_keys() {
    let Some(env) = integration_env().await else {
        return;
    };
    let now = Utc::now();
    let (project_id, budgeted_key_id) = insert_budgeted_key(&env.store, Some(10.0), None).await;
    insert_usage(
        &env.store,
        budgeted_key_id,
        project_id,
        "positive",
        Some(2.5),
        now,
    )
    .await;
    insert_usage(
        &env.store,
        budgeted_key_id,
        project_id,
        "zero",
        Some(0.0),
        now,
    )
    .await;
    insert_usage(
        &env.store,
        budgeted_key_id,
        project_id,
        "negative",
        Some(-8.0),
        now,
    )
    .await;
    insert_usage(&env.store, budgeted_key_id, project_id, "null", None, now).await;
    let (_, unbudgeted_key_id) = insert_budgeted_key(&env.store, None, None).await;

    seed_from_postgres(&env.store, &env.redis, now).await;

    let allowed = env
        .redis
        .check_budget(budgeted_key_id, Some(10.0), None, now)
        .await
        .expect("check budget");
    assert!(matches!(allowed, BudgetDecision::Allowed(_)));
    if let BudgetDecision::Allowed(state) = allowed {
        assert!((state.daily_spend_usd - 2.5).abs() < f64::EPSILON);
    }

    let seeds = env.store.budget_counter_seeds(now).await.expect("seeds");
    assert!(!seeds.iter().any(|seed| seed.key_id == unbudgeted_key_id));
}

#[tokio::test]
async fn rehydration_preserves_existing_budget_reservations() {
    let Some(env) = integration_env().await else {
        return;
    };
    let now = Utc::now();
    let (project_id, key_id) = insert_budgeted_key(&env.store, Some(10.0), Some(20.0)).await;
    insert_usage(
        &env.store,
        key_id,
        project_id,
        "reservation-history",
        Some(1.0),
        now,
    )
    .await;
    env.redis
        .reserve_budget(key_id, "req-reservation", 0.5, now)
        .await
        .expect("reserve budget");

    seed_from_postgres(&env.store, &env.redis, now).await;
    env.redis
        .reconcile_budget_reservation(key_id, "req-reservation", 0.75, now)
        .await
        .expect("reconcile reservation");

    let decision = env
        .redis
        .check_budget(key_id, Some(10.0), Some(20.0), now)
        .await
        .expect("check budget");
    if let BudgetDecision::Allowed(state) = decision {
        assert!((state.daily_spend_usd - 1.25).abs() < 0.000_001);
    } else {
        panic!("expected budget to remain allowed");
    }
}

#[tokio::test]
async fn tpm_counter_is_shared_and_returns_retry_hint() {
    let Some(env) = integration_env().await else {
        return;
    };
    let now = Utc::now() + Duration::seconds(1);
    let key_id = Uuid::new_v4();
    let mut connection = env
        .redis_client
        .get_multiplexed_async_connection()
        .await
        .expect("redis connection");
    let tpm_key = gateway_core::rate_limits::token_rate_limit_key(key_id, now);
    let _: usize = connection.del(&tpm_key).await.expect("delete tpm key");

    let first = env
        .redis
        .check_token_rate_limit(key_id, Some(10), 6, now)
        .await
        .expect("first tpm");
    assert!(matches!(first, RateLimitDecision::Allowed { count: 6 }));
    let second = env
        .redis
        .check_token_rate_limit(key_id, Some(10), 6, now)
        .await
        .expect("second tpm");
    assert!(matches!(
        second,
        RateLimitDecision::Exceeded {
            count: 12,
            retry_after_seconds: Some(_)
        }
    ));
}
