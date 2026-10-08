use chrono::Utc;
use gateway_core::{
    AdminServiceStore, BudgetDecision, BudgetStore, ServiceCreateRequest, ServicePatchRequest,
    UsageQuery, UsageQueryStore, UsageRecorder,
};
use gateway_store::{PostgresStore, RedisControlState};
use serde_json::{json, Value};
use uuid::Uuid;

fn request<T: serde::de::DeserializeOwned>(value: Value) -> T {
    serde_json::from_value(value).expect("valid test request")
}

async fn store() -> Option<(PostgresStore, sqlx::Transaction<'static, sqlx::Postgres>)> {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("skipping historical pricing integration: DATABASE_URL is not set");
        return None;
    };
    let store = PostgresStore::connect(&url)
        .await
        .expect("migrate test store");
    // The rollback fixture briefly installs a table constraint. Serialize it
    // with the repository's other dependency-backed control-plane fixtures.
    let mut lock = store.pool().begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(82120260808)")
        .execute(&mut *lock)
        .await
        .unwrap();
    Some((store, lock))
}

async fn create_service(store: &PostgresStore, name: &str) {
    store
        .create_service(request::<ServiceCreateRequest>(json!({
            "name": name, "upstream_base_url": "http://pricing.internal",
            "cost_mode": "fixed", "estimated_cost_usd": 0.1
        })))
        .await
        .expect("create service");
}

async fn create_key(store: &PostgresStore) -> Uuid {
    let key = Uuid::new_v4();
    sqlx::query("INSERT INTO api_keys (id, owner_type, project_id, key_prefix, key_hash) VALUES ($1, 'individual', NULL, $2, 'test-hash')")
        .bind(key).bind(format!("rk_live_{}", key.simple()))
        .execute(store.pool()).await.expect("create test key");
    key
}

#[tokio::test]
async fn historical_reporting_updates_preserve_original_budgets_and_are_atomic() {
    let Some((store, _lock)) = store().await else {
        return;
    };
    let key = create_key(&store).await;
    let name = format!("pricing-{}", Uuid::new_v4().simple());
    let other = format!("pricing-{}", Uuid::new_v4().simple());
    create_service(&store, &name).await;
    create_service(&store, &other).await;
    let rules = json!([{
        "name": "premium", "json_pointer": "/tier", "equals": "premium",
        "cost_mode": "fixed", "estimated_cost_usd": 0.3
    }]);
    let endpoints = json!([
        {"method":"POST", "path_template":"/run", "operation_id":"run", "cost_mode":"fixed", "estimated_cost_usd":0.2},
        {"method":"GET", "path_template":"/health", "operation_id":"health", "cost_mode":"none"}
    ]);
    store
        .patch_service(
            &name,
            request(json!({"pricing_rules":rules, "endpoint_pricing_rules":endpoints})),
        )
        .await
        .expect("set original rules");
    let registration = gateway_core::ServiceRegistryLookup::service_registration(&store, &name)
        .await
        .unwrap()
        .unwrap();
    let body_fingerprint = gateway_core::resolve_service_cost_from_value(
        &json!({"tier":"premium"}),
        registration.cost_mode,
        registration.estimated_cost_usd,
        &registration.pricing_rules,
    )
    .pricing_rule_fingerprint;
    let traffic = Uuid::new_v4();
    let default_id = Uuid::new_v4();
    let now = Utc::now();
    let mut records = Vec::new();
    for (label, cost, source, mode, rule, method, path, service) in [
        (
            "default",
            Some(0.1),
            Some("service_default_fixed"),
            Some("fixed"),
            None,
            "POST",
            "/other",
            &name,
        ),
        (
            "body",
            Some(0.3),
            Some("service_pricing_rule_fixed"),
            Some("fixed"),
            Some("premium"),
            "POST",
            "/other",
            &name,
        ),
        (
            "endpoint",
            Some(0.2),
            Some("service_pricing_rule_fixed"),
            Some("fixed"),
            Some("run"),
            "POST",
            "/run",
            &name,
        ),
        (
            "upstream",
            Some(0.7),
            Some("service_default_passthrough"),
            Some("passthrough"),
            None,
            "POST",
            "/other",
            &name,
        ),
        (
            "free",
            None,
            Some("none"),
            Some("none"),
            Some("health"),
            "GET",
            "/health",
            &name,
        ),
        (
            "unknown",
            Some(0.4),
            Some("service_pricing_rule_fixed"),
            Some("fixed"),
            Some("removed"),
            "POST",
            "/other",
            &name,
        ),
        ("denied", None, None, None, None, "POST", "/other", &name),
        (
            "other",
            Some(0.9),
            Some("service_default_fixed"),
            Some("fixed"),
            None,
            "POST",
            "/other",
            &other,
        ),
    ] {
        records.push(json!({
            "id": if label == "default" { default_id } else { Uuid::new_v4() },
            "request_id": format!("{name}-{label}"), "cost":cost, "source":source,
            "mode":mode, "rule":rule, "method":method, "path":path, "service":service,
            "fingerprint": match label {
                "body" => body_fingerprint.clone(),
                "endpoint" | "free" => gateway_core::resolve_endpoint_pricing_rule(
                    &method.parse().unwrap(), path, &registration.endpoint_pricing_rules,
                ).unwrap().pricing_rule_fingerprint,
                _ => None,
            },
            "traffic_id": if label == "default" { Some(traffic) } else { None },
        }));
    }
    sqlx::query(r#"
        INSERT INTO usage_events (id, request_id, key_id, project_id, route, provider,
            status, status_code, latency_ms, estimated_cost, cost_source, cost_mode,
            pricing_rule_name, service_name, http_method, endpoint_path, diagnostics, created_at, pricing_rule_fingerprint)
        SELECT c.id, c.request_id, $2, NULL, '/services/*', 'internal-service', 'success',
            200, 1, c.cost, c.source, c.mode, c.rule, c.service, c.method, c.path,
            jsonb_build_object('traffic_id', c.traffic_id), $3, c.fingerprint
        FROM jsonb_to_recordset($1::jsonb) AS c(id uuid, request_id text, cost double precision,
            source text, mode text, rule text, service text, method text, path text, traffic_id uuid, fingerprint text)
    "#).bind(sqlx::types::Json(records)).bind(key).bind(now)
        .execute(store.pool()).await.expect("record original costs");
    sqlx::query(
        r#"INSERT INTO request_traffic
        (id, instance_id, request_id, started_at, key_id, service, client_status, failed, record)
        VALUES ($1, 'pricing-test', $2, $3, $4, $5, 200, false, $6)"#,
    )
    .bind(traffic)
    .bind(format!("{name}-default"))
    .bind(now)
    .bind(key)
    .bind(&name)
    .bind(json!({"usage":{"estimated_cost_usd":0.1,"cost_source":"service_default_fixed"}}))
    .execute(store.pool())
    .await
    .expect("record correlated traffic snapshot");
    let original = store.committed_budget_spend(key, now).await.unwrap();
    assert!((original.daily_spend_usd - 2.6).abs() < 1e-9);

    let future_only = store
        .patch_service(&name, request(json!({"estimated_cost_usd":0.15})))
        .await
        .unwrap()
        .unwrap();
    assert!(future_only.historical_usage_repricing.is_none());
    let unchanged: f64 = sqlx::query_scalar(
        "SELECT estimated_cost::double precision FROM usage_events WHERE id = $1",
    )
    .bind(default_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(unchanged, 0.1);

    let updated = store.patch_service(&name, request(json!({
        "estimated_cost_usd":0.0002, "reprice_existing_usage":true,
        "pricing_rules":[{"name":"premium","json_pointer":"/tier","equals":"premium","cost_mode":"fixed","estimated_cost_usd":0.0005}],
        "endpoint_pricing_rules":[
            {"method":"POST","path_template":"/run","operation_id":"run","cost_mode":"fixed","estimated_cost_usd":0.0003},
            {"method":"GET","path_template":"/health","operation_id":"health","cost_mode":"fixed","estimated_cost_usd":0.0004}
        ]
    }))).await.unwrap().unwrap();
    let summary = updated.historical_usage_repricing.unwrap();
    assert_eq!(summary.updated_requests, 4);
    assert_eq!(summary.unchanged_requests, 3);
    let report = store
        .usage_summary(UsageQuery {
            key_id: Some(key),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!((report.estimated_cost_usd.unwrap() - 2.0014).abs() < 1e-9);
    assert_eq!(
        store.committed_budget_spend(key, now).await.unwrap(),
        original
    );
    let traffic_cost: f64 = sqlx::query_scalar("SELECT (record #>> '{usage,estimated_cost_usd}')::double precision FROM request_traffic WHERE id = $1")
        .bind(traffic).fetch_one(store.pool()).await.unwrap();
    assert_eq!(traffic_cost, 0.0002);
    let original_free: f64 = sqlx::query_scalar(
        "SELECT budget_estimated_cost::double precision FROM usage_events WHERE request_id = $1",
    )
    .bind(format!("{name}-free"))
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(
        original_free, 0.0,
        "new reporting charge does not become a budget charge"
    );

    // Force a store failure after the service UPDATE to prove transaction rollback.
    let constraint = format!("pricing_{}", Uuid::new_v4().simple());
    sqlx::query(&format!("ALTER TABLE usage_events ADD CONSTRAINT {constraint} CHECK (service_name <> '{name}' OR estimated_cost IS NULL OR estimated_cost >= 0.00001) NOT VALID"))
        .execute(store.pool()).await.unwrap();
    let failed = store
        .patch_service(
            &name,
            request(json!({"estimated_cost_usd":0,"reprice_existing_usage":true})),
        )
        .await;
    sqlx::query(&format!(
        "ALTER TABLE usage_events DROP CONSTRAINT {constraint}"
    ))
    .execute(store.pool())
    .await
    .unwrap();
    assert!(failed.is_err());
    assert_eq!(
        store
            .get_service(&name)
            .await
            .unwrap()
            .unwrap()
            .estimated_cost_usd,
        Some(0.0002)
    );
    assert_eq!(
        store
            .usage_summary(UsageQuery {
                key_id: Some(key),
                ..Default::default()
            })
            .await
            .unwrap()
            .estimated_cost_usd,
        report.estimated_cost_usd
    );

    let repeated = store
        .patch_service(
            &name,
            request::<ServicePatchRequest>(
                json!({"estimated_cost_usd":0.00025,"reprice_existing_usage":true}),
            ),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        repeated
            .historical_usage_repricing
            .unwrap()
            .updated_requests,
        1
    );
    assert_eq!(
        store.committed_budget_spend(key, now).await.unwrap(),
        original
    );
    let seed = store
        .budget_counter_seeds(now)
        .await
        .unwrap()
        .into_iter()
        .find(|seed| seed.key_id == key)
        .unwrap();
    assert_eq!(seed.daily_spend_usd, original.daily_spend_usd);
    assert_eq!(seed.monthly_spend_usd, original.monthly_spend_usd);
    if let Ok(url) = std::env::var("REDIS_URL") {
        let redis = RedisControlState::new(&url).unwrap();
        redis
            .seed_budget_counters(key, seed.daily_spend_usd, seed.monthly_spend_usd, now)
            .await
            .unwrap();
        assert!(
            matches!(redis.check_budget(key, Some(2.5), Some(2.5), now).await.unwrap(), BudgetDecision::Exceeded(state) if state == original)
        );
    }
}

#[tokio::test]
async fn historical_repricing_excludes_requests_recorded_after_its_snapshot() {
    let Some((store, _lock)) = store().await else {
        return;
    };
    let key = create_key(&store).await;
    let name = format!("pricing-{}", Uuid::new_v4().simple());
    create_service(&store, &name).await;
    let original_id = Uuid::new_v4();
    let late_id = Uuid::new_v4();
    let insert = "INSERT INTO usage_events (id, request_id, key_id, route, provider, status, status_code, latency_ms, estimated_cost, cost_source, cost_mode, service_name) VALUES ($1, $2, $3, '/services/*', 'internal-service', 'success', 200, 1, 0.1, 'service_default_fixed', 'fixed', $4)";
    sqlx::query(insert)
        .bind(original_id)
        .bind(format!("{name}-original"))
        .bind(key)
        .bind(&name)
        .execute(store.pool())
        .await
        .unwrap();

    // Block the historical row so we can commit a late request after the
    // repricing transaction has taken its snapshot, without a timing race.
    let mut blocker = store.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM usage_events WHERE id = $1 FOR UPDATE")
        .bind(original_id)
        .execute(&mut *blocker)
        .await
        .unwrap();
    let transaction_id: String = sqlx::query_scalar("SELECT pg_current_xact_id()::text")
        .fetch_one(&mut *blocker)
        .await
        .unwrap();
    let updating = store.clone();
    let updating_name = name.clone();
    let update = tokio::spawn(async move {
        updating
            .patch_service(
                &updating_name,
                request(json!({
                    "estimated_cost_usd":0.0002, "reprice_existing_usage":true
                })),
            )
            .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let waiting: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_locks WHERE locktype = 'transactionid' AND transactionid::text = $1 AND NOT granted)")
                .bind(&transaction_id).fetch_one(store.pool()).await.unwrap();
            if waiting { break; }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }).await.expect("repricing waits for the locked historical row");
    sqlx::query(insert)
        .bind(late_id)
        .bind(format!("{name}-late"))
        .bind(key)
        .bind(&name)
        .execute(store.pool())
        .await
        .unwrap();
    blocker.commit().await.unwrap();
    let summary = update
        .await
        .unwrap()
        .unwrap()
        .unwrap()
        .historical_usage_repricing
        .unwrap();
    assert_eq!(summary.updated_requests, 1);
    assert_eq!(summary.unchanged_requests, 0);
    let late: (f64, Option<f64>) = sqlx::query_as("SELECT estimated_cost::double precision, budget_estimated_cost::double precision FROM usage_events WHERE id = $1")
        .bind(late_id).fetch_one(store.pool()).await.unwrap();
    assert_eq!(
        late,
        (0.1, None),
        "an in-flight request retains its resolved original charge"
    );
}

#[tokio::test]
async fn historical_repricing_processes_more_than_one_bounded_batch() {
    let Some((store, _lock)) = store().await else {
        return;
    };
    let key = create_key(&store).await;
    let name = format!("pricing-{}", Uuid::new_v4().simple());
    create_service(&store, &name).await;
    sqlx::query(r#"INSERT INTO usage_events (request_id, key_id, project_id, route,
        provider, status, status_code, latency_ms, estimated_cost, cost_source, cost_mode, service_name)
        SELECT $1 || '-' || n, $2, NULL, '/services/*', 'internal-service', 'success',
            200, 1, 0.1, 'service_default_fixed', 'fixed', $1 FROM generate_series(1, 1002) n"#)
        .bind(&name).bind(key).execute(store.pool()).await.unwrap();
    let updated = store
        .patch_service(
            &name,
            request(json!({"estimated_cost_usd":0.0002,"reprice_existing_usage":true})),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated.historical_usage_repricing.unwrap().updated_requests,
        1002
    );
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM usage_events WHERE service_name = $1 AND estimated_cost = 0.0002 AND budget_estimated_cost = 0.1")
        .bind(&name).fetch_one(store.pool()).await.unwrap();
    assert_eq!(count, 1002);
}

async fn record_rule_usage(store: &PostgresStore, name: &str, key: Uuid, tier: &str) -> Uuid {
    let registration = gateway_core::ServiceRegistryLookup::service_registration(store, name)
        .await
        .unwrap()
        .unwrap();
    let resolved = gateway_core::resolve_service_cost_from_value(
        &json!({"tier":tier}),
        registration.cost_mode,
        registration.estimated_cost_usd,
        &registration.pricing_rules,
    );
    let key_context = gateway_core::AuthenticatedKey {
        key_id: key,
        project_id: None,
        key_prefix: "test".into(),
    };
    let id = Uuid::new_v4().to_string();
    let event = gateway_core::UsageEvent::new(
        &id,
        &key_context,
        gateway_core::Route::ServiceWildcard,
        None,
        200,
        1,
        Utc::now(),
    )
    .with_provider(gateway_core::Provider::InternalService)
    .with_estimated_cost_usd(resolved.estimated_cost_usd)
    .with_cost_metadata(
        Some("service_pricing_rule_fixed".into()),
        Some(resolved.cost_mode),
        resolved.pricing_rule_name,
    )
    .with_pricing_rule_fingerprint(resolved.pricing_rule_fingerprint)
    .with_service_name(Some(name.into()))
    .with_endpoint_context(Some("POST".into()), Some("/run".into()), None);
    store.insert_usage_event(&event).await.unwrap();
    sqlx::query_scalar("SELECT id FROM usage_events WHERE request_id = $1")
        .bind(id)
        .fetch_one(store.pool())
        .await
        .unwrap()
}

#[tokio::test]
async fn named_rule_repricing_requires_per_event_selector_proof_across_new_only_edits() {
    let Some((store, _lock)) = store().await else {
        return;
    };
    let key = create_key(&store).await;
    let name = format!("proof-{}", Uuid::new_v4().simple());
    create_service(&store, &name).await;
    let rule = |tier: &str, cost: f64| {
        json!([{"name":"premium", "json_pointer":"/tier",
        "equals":tier, "cost_mode":"fixed", "estimated_cost_usd":cost}])
    };
    store
        .patch_service(&name, request(json!({"pricing_rules":rule("old", 0.3)})))
        .await
        .unwrap();
    let old_id = record_rule_usage(&store, &name, key, "old").await;
    let legacy_id = record_rule_usage(&store, &name, key, "old").await;
    sqlx::query("UPDATE usage_events SET pricing_rule_fingerprint = NULL WHERE id = $1")
        .bind(legacy_id)
        .execute(store.pool())
        .await
        .unwrap();
    store
        .patch_service(&name, request(json!({"pricing_rules":rule("new", 0.4)})))
        .await
        .unwrap();
    let new_id = record_rule_usage(&store, &name, key, "new").await;
    let original_budget = store.committed_budget_spend(key, Utc::now()).await.unwrap();
    for cost in [0.0002, 0.0003] {
        let summary = store
            .patch_service(
                &name,
                request(json!({
                    "pricing_rules":rule("new", cost), "reprice_existing_usage":true
                })),
            )
            .await
            .unwrap()
            .unwrap()
            .historical_usage_repricing
            .unwrap();
        assert_eq!(summary.updated_requests, 1);
        assert_eq!(summary.unchanged_requests, 2);
        for id in [old_id, legacy_id] {
            let row: (f64, Option<f64>) = sqlx::query_as("SELECT estimated_cost::double precision, budget_estimated_cost::double precision FROM usage_events WHERE id = $1")
                .bind(id).fetch_one(store.pool()).await.unwrap();
            assert_eq!(row, (0.3, None));
        }
        let row: (f64, f64, String) = sqlx::query_as("SELECT estimated_cost::double precision, budget_estimated_cost::double precision, pricing_rule_fingerprint FROM usage_events WHERE id = $1")
            .bind(new_id).fetch_one(store.pool()).await.unwrap();
        assert_eq!((row.0, row.1), (cost, 0.4));
        assert!(row.2.starts_with("v1:body:"));
        assert!(!row.2.contains("premium"), "only the hash is persisted");
        assert_eq!(
            store.committed_budget_spend(key, Utc::now()).await.unwrap(),
            original_budget
        );
    }
    // The same price remains idempotent; unknown proof versions are skipped.
    sqlx::query("UPDATE usage_events SET pricing_rule_fingerprint = 'v2:unknown' WHERE id = $1")
        .bind(new_id)
        .execute(store.pool())
        .await
        .unwrap();
    let summary = store
        .patch_service(
            &name,
            request(json!({
                "pricing_rules":rule("new", 0.0004), "reprice_existing_usage":true
            })),
        )
        .await
        .unwrap()
        .unwrap()
        .historical_usage_repricing
        .unwrap();
    assert_eq!(summary.updated_requests, 0);
    assert_eq!(summary.unchanged_requests, 3);
}

#[tokio::test]
async fn minimum_service_price_survives_recording_repricing_and_budget_recovery() {
    let Some((store, _lock)) = store().await else {
        return;
    };
    let key = create_key(&store).await;
    let name = format!("precision-{}", Uuid::new_v4().simple());
    create_service(&store, &name).await;
    let rules = |cost: f64| {
        json!([{"name":"tiny", "json_pointer":"/tier", "equals":"tiny",
        "cost_mode":"fixed", "estimated_cost_usd":cost}])
    };
    store
        .patch_service(&name, request(json!({"pricing_rules":rules(0.00000001)})))
        .await
        .unwrap();
    let id = record_rule_usage(&store, &name, key, "tiny").await;
    assert!(store
        .patch_service(&name, request(json!({"pricing_rules":rules(0.000000001)})))
        .await
        .is_err());
    store
        .patch_service(
            &name,
            request(json!({"pricing_rules":rules(0.00000002), "reprice_existing_usage":true})),
        )
        .await
        .unwrap();
    let row: (f64, f64) = sqlx::query_as("SELECT estimated_cost::double precision, budget_estimated_cost::double precision FROM usage_events WHERE id = $1")
        .bind(id).fetch_one(store.pool()).await.unwrap();
    assert_eq!(row, (0.00000002, 0.00000001));
    let now = Utc::now();
    let recovered = store.committed_budget_spend(key, now).await.unwrap();
    assert_eq!(recovered.daily_spend_usd, 0.00000001);
    assert_eq!(recovered.monthly_spend_usd, 0.00000001);
    if let Ok(url) = std::env::var("REDIS_URL") {
        let redis = RedisControlState::new(&url).unwrap();
        redis.add_budget_spend(key, 0.00000001, now).await.unwrap();
        let live = redis
            .check_budget(key, Some(1.0), Some(1.0), now)
            .await
            .unwrap();
        let mut client = redis::Client::open(url)
            .unwrap()
            .get_multiplexed_async_connection()
            .await
            .unwrap();
        let keys = [
            format!("budget:day:{key}:{}", now.format("%Y%m%d")),
            format!("budget:month:{key}:{}", now.format("%Y%m")),
        ];
        let _: i64 = redis::cmd("DEL")
            .arg(&keys)
            .query_async(&mut client)
            .await
            .unwrap();
        redis
            .seed_committed_budget(key, recovered, now)
            .await
            .unwrap();
        assert_eq!(
            redis
                .check_budget(key, Some(1.0), Some(1.0), now)
                .await
                .unwrap(),
            live
        );
    }
}

#[tokio::test]
async fn foundry_patch_uses_one_connection_and_invalid_provider_rolls_back() {
    use gateway_core::{AdminProviderConfigStore, GatewayError};
    let Some((store, _lock)) = store().await else {
        return;
    };
    let name = format!("foundry-{}", Uuid::new_v4().simple());
    let provider = store
        .create_provider_config(request(json!({
            "provider":"azure-foundry", "name":name,
            "base_url":"https://example.services.ai.azure.com/api/projects/test",
            "foundry":{"method":"managed_identity"}
        })))
        .await
        .unwrap();
    store
        .create_service(request::<ServiceCreateRequest>(json!({
            "name":name, "cost_mode":"fixed", "estimated_cost_usd":0.1,
            "foundry":{"mode":"endpoint_passthrough","provider_id":provider.id}
        })))
        .await
        .unwrap();
    let url = std::env::var("DATABASE_URL").unwrap();
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(std::time::Duration::from_secs(2))
        .connect(&url)
        .await
        .unwrap();
    let single = PostgresStore::new(pool);
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        single.patch_service(
            &name,
            request(json!({"estimated_cost_usd":0.0002,"reprice_existing_usage":true})),
        ),
    )
    .await
    .expect("single-connection edit completes")
    .unwrap()
    .unwrap();
    assert_eq!(result.estimated_cost_usd, Some(0.0002));
    assert!(result.historical_usage_repricing.is_some());
    let non_foundry = store.create_provider_config(request(json!({
        "provider":"internal-service", "name":format!("wrong-{name}"), "base_url":"http://wrong.internal"
    }))).await.unwrap();
    for id in [Uuid::new_v4(), non_foundry.id] {
        let result = single
            .patch_service(
                &name,
                request(json!({"estimated_cost_usd":0.9,
            "foundry":{"mode":"endpoint_passthrough", "provider_id":id}})),
            )
            .await;
        assert_eq!(result, Err(GatewayError::InvalidFoundryConfiguration));
        let saved = single.get_service(&name).await.unwrap().unwrap();
        assert_eq!(saved.estimated_cost_usd, Some(0.0002));
        assert_eq!(saved.foundry.unwrap().provider_id(), provider.id);
    }
    single.pool().close().await;

    // Saturate a three-connection pool with edits waiting on the same row.
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(3)
        .acquire_timeout(std::time::Duration::from_secs(30))
        .connect(&url)
        .await
        .unwrap();
    let concurrent = PostgresStore::new(pool);
    let mut blocker = store.pool().begin().await.unwrap();
    sqlx::query("SELECT name FROM service_registrations WHERE name = $1 FOR UPDATE")
        .bind(&name)
        .execute(&mut *blocker)
        .await
        .unwrap();
    let xid: String = sqlx::query_scalar("SELECT pg_current_xact_id()::text")
        .fetch_one(&mut *blocker)
        .await
        .unwrap();
    let mut updates = Vec::new();
    for _ in 0..3 {
        let updating = concurrent.clone();
        let updating_name = name.clone();
        updates.push(tokio::spawn(async move {
            updating
                .patch_service(
                    &updating_name,
                    request(json!({"estimated_cost_usd":0.0003})),
                )
                .await
        }));
    }
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pg_locks WHERE locktype = 'transactionid' AND transactionid::text = $1 AND NOT granted")
                .bind(&xid).fetch_one(store.pool()).await.unwrap();
            if count > 0 && concurrent.pool().size() == 3 && concurrent.pool().num_idle() == 0 { break; }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }).await.expect("all edit connections wait on the row lock");
    blocker.commit().await.unwrap();
    let mut succeeded = 0;
    for update in updates {
        let result = tokio::time::timeout(std::time::Duration::from_secs(5), update)
            .await
            .expect("pool-saturated edit terminates")
            .unwrap();
        match result {
            Ok(Some(_)) => succeeded += 1,
            Err(GatewayError::StoreUnavailable) => {} // Existing REPEATABLE READ conflict.
            other => panic!("unexpected edit result: {other:?}"),
        }
    }
    assert!(
        succeeded >= 1,
        "the lock holder can validate its provider without another connection"
    );
    concurrent.pool().close().await;
}

#[tokio::test]
async fn supported_large_service_prices_round_trip_without_float_cast_rounding() {
    let Some((store, _lock)) = store().await else {
        return;
    };
    let key = create_key(&store).await;
    let name = format!("large-price-{}", Uuid::new_v4().simple());
    create_service(&store, &name).await;
    let rules = |cost: f64| {
        json!([{"name":"large", "json_pointer":"/tier", "equals":"large",
        "cost_mode":"fixed", "estimated_cost_usd":cost}])
    };
    let initial = 10_000_000.12345678;
    let upper = 999_999_999_999.999_9;
    store
        .patch_service(&name, request(json!({"pricing_rules":rules(initial)})))
        .await
        .unwrap();
    let id = record_rule_usage(&store, &name, key, "large").await;
    let text: String =
        sqlx::query_scalar("SELECT estimated_cost::text FROM usage_events WHERE id = $1")
            .bind(id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(text, "10000000.12345678");
    store
        .patch_service(
            &name,
            request(json!({"pricing_rules":rules(upper), "reprice_existing_usage":true})),
        )
        .await
        .unwrap();
    let row: (String, String) = sqlx::query_as(
        "SELECT estimated_cost::text, budget_estimated_cost::text FROM usage_events WHERE id = $1",
    )
    .bind(id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(
        row,
        ("999999999999.99990000".into(), "10000000.12345678".into())
    );
    // The direct writer must also handle the largest representable configured price.
    record_rule_usage(&store, &name, key, "large").await;
}
