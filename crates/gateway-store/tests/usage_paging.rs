use chrono::{Duration, Utc};
use gateway_core::{
    observability::{UsageSection, UsageSectionKind},
    AdminServiceStore, UsageQuery, UsageQueryStore,
};
use gateway_store::PostgresStore;
use uuid::Uuid;

const INSERT_EVENT: &str = "INSERT INTO usage_events (id, key_id, project_id, request_id, created_at, total_tokens, route, provider, status, status_code, latency_ms, estimated_cost) VALUES ($1,$2,$3,$4,$5,$6,'/v1/messages','litellm','success',200,10,1.25)";
const INSERT_GUARDRAIL: &str = "INSERT INTO guardrail_execution_events (request_id,key_id,project_id,guardrail_name,mode,action,failure_policy,latency_ms) VALUES ($1,$2,$3,'test','pre_call','block','fail_closed',1)";

struct Fixture {
    store: PostgresStore,
    project: Uuid,
    key: Uuid,
    _database_lock: sqlx::Transaction<'static, sqlx::Postgres>,
}

impl Fixture {
    async fn new() -> Option<Self> {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping Usage paging integration test: DATABASE_URL is not set");
            return None;
        };
        let store = PostgresStore::connect(&url)
            .await
            .expect("connect disposable PostgreSQL");
        // Global Admin API tests read the entire ledger. Serialize synthetic
        // boundary values with those tests, and release even after a panic.
        let mut database_lock = store.pool().begin().await.unwrap();
        sqlx::query("SELECT pg_advisory_xact_lock(82120260808)")
            .execute(&mut *database_lock)
            .await
            .unwrap();
        let project = Uuid::new_v4();
        let key = Uuid::new_v4();
        sqlx::query("INSERT INTO projects (id,name) VALUES ($1,$2)")
            .bind(project)
            .bind(format!("usage-{project}"))
            .execute(store.pool())
            .await
            .unwrap();
        sqlx::query("INSERT INTO api_keys (id,owner_type,project_id,key_prefix,key_hash) VALUES ($1,'project',$2,$3,'unused-test-hash')").bind(key).bind(project).bind(format!("usage-{key}")).execute(store.pool()).await.unwrap();
        Some(Self {
            store,
            project,
            key,
            _database_lock: database_lock,
        })
    }

    fn query(&self) -> UsageQuery {
        UsageQuery {
            project_id: Some(self.project),
            ..Default::default()
        }
    }

    async fn insert(&self, request: &str, at: chrono::DateTime<Utc>, tokens: i64) {
        sqlx::query(INSERT_EVENT)
            .bind(Uuid::new_v4())
            .bind(self.key)
            .bind(self.project)
            .bind(request)
            .bind(at)
            .bind(tokens)
            .execute(self.store.pool())
            .await
            .unwrap();
    }

    async fn cleanup(self) {
        sqlx::query("DELETE FROM guardrail_execution_events WHERE key_id=$1 OR project_id=$2")
            .bind(self.key)
            .bind(self.project)
            .execute(self.store.pool())
            .await
            .unwrap();
        sqlx::query("DELETE FROM usage_events WHERE key_id=$1")
            .bind(self.key)
            .execute(self.store.pool())
            .await
            .unwrap();
        sqlx::query("DELETE FROM api_keys WHERE id=$1")
            .bind(self.key)
            .execute(self.store.pool())
            .await
            .unwrap();
        sqlx::query("DELETE FROM projects WHERE id=$1")
            .bind(self.project)
            .execute(self.store.pool())
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn usage_summary_default_cost_threshold_preserves_numeric_boundaries() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    sqlx::query(
        r#"
        INSERT INTO usage_events (
            request_id, key_id, project_id, route, provider, status,
            status_code, latency_ms, estimated_cost
        )
        SELECT 'cost-boundary-' || n, $1, $2, '/test/summary/cost',
            'litellm', 'success', 200, 0, cost
        FROM unnest(ARRAY[
            NULL, 0, 0.99999999, 1.0, 1.00000001, 999999999999.99999999
        ]::numeric[]) WITH ORDINALITY AS sample(cost, n)
        "#,
    )
    .bind(f.key)
    .bind(f.project)
    .execute(f.store.pool())
    .await
    .unwrap();
    let expected: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FILTER (WHERE estimated_cost >= $1)::bigint FROM usage_events WHERE project_id = $2",
    )
    .bind(1.0_f64)
    .bind(f.project)
    .fetch_one(f.store.pool())
    .await
    .unwrap();
    assert_eq!(expected, 3);
    let summary = f.store.usage_summary(f.query()).await.unwrap();
    assert_eq!(summary.request_count, 6);
    assert_eq!(summary.expensive_request_count, expected);
    let mut filtered = f.query();
    filtered.min_cost_usd = Some(1.0);
    let summary = f.store.usage_summary(filtered).await.unwrap();
    assert_eq!(summary.request_count, expected);
    assert_eq!(summary.expensive_request_count, expected);
    f.cleanup().await;
}

#[tokio::test]
async fn usage_summary_matches_released_aggregates_and_exact_percentile() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let cases = [
        vec![],
        vec![42],
        vec![17; 40],
        vec![10, 31],
        vec![0, 0, 1_000_000],
        vec![-500, 0, 1, 19, 400, 400, 1_000_003],
        (0..21).collect(),
        (0..257).map(|n| (n * 7919) % 104729).collect(),
        vec![
            9_007_199_254_740_991,
            9_007_199_254_740_992,
            9_007_199_254_740_993,
            9_007_199_254_740_996,
        ],
        vec![i64::MIN, i64::MAX],
        vec![i64::MAX, i64::MAX, i64::MIN],
    ];
    for (case, latencies) in cases.into_iter().enumerate() {
        let route = format!("/test/summary/{case}");
        sqlx::query(
            r#"
            INSERT INTO usage_events (
                request_id, key_id, project_id, route, provider, status,
                status_code, latency_ms, input_tokens, output_tokens,
                total_tokens, estimated_cost, fallback_count
            )
            SELECT 'summary-' || n, $1, $2, $3, 'litellm',
                CASE n % 3 WHEN 0 THEN 'failure' WHEN 1 THEN 'success' ELSE 'other' END,
                CASE n % 4 WHEN 0 THEN 403 WHEN 1 THEN 429 WHEN 2 THEN 402 ELSE 200 END,
                latency,
                CASE WHEN n % 2 = 0 THEN n * 3 END,
                CASE WHEN n % 3 = 0 THEN n * 5 END,
                n * 11,
                CASE WHEN n % 4 != 1 THEN (n % 7)::numeric / 8 END,
                (n % 3)::integer
            FROM unnest($4::bigint[]) WITH ORDINALITY AS sample(latency, n)
            "#,
        )
        .bind(f.key)
        .bind(f.project)
        .bind(&route)
        .bind(&latencies)
        .execute(f.store.pool())
        .await
        .unwrap();
        // Keep PostgreSQL's released ordered-set aggregate as the reference,
        // including its floating-point rank/interpolation rounding.
        type ReferenceSummary = (
            i64,
            i64,
            i64,
            i64,
            i64,
            i64,
            f64,
            i64,
            Option<f64>,
            i64,
            i64,
            i64,
            i64,
            i64,
        );
        let expected: ReferenceSummary = sqlx::query_as(
            r#"
            SELECT COUNT(*)::bigint,
                COUNT(*) FILTER (WHERE status = 'success')::bigint,
                COUNT(*) FILTER (WHERE status = 'failure')::bigint,
                COALESCE(SUM(input_tokens), 0)::bigint,
                COALESCE(SUM(output_tokens), 0)::bigint,
                COALESCE(SUM(total_tokens), 0)::bigint,
                COALESCE(SUM(estimated_cost), 0)::double precision,
                COALESCE(SUM(latency_ms), 0)::bigint,
                percentile_cont(0.95) WITHIN GROUP (ORDER BY latency_ms),
                COALESCE(SUM(fallback_count), 0)::bigint,
                COUNT(*) FILTER (WHERE status_code = 403)::bigint,
                COUNT(*) FILTER (WHERE status_code = 429)::bigint,
                COUNT(*) FILTER (WHERE status_code = 402)::bigint,
                COUNT(*) FILTER (WHERE estimated_cost >= 0.25)::bigint
            FROM usage_events WHERE project_id = $1 AND route = $2
            "#,
        )
        .bind(f.project)
        .bind(&route)
        .fetch_one(f.store.pool())
        .await
        .unwrap();
        let mut query = f.query();
        query.route = Some(route);
        let actual = f.store.usage_summary(query.clone()).await.unwrap();
        assert_eq!(actual.request_count, expected.0, "case {case}");
        assert_eq!(actual.success_count, expected.1, "case {case}");
        assert_eq!(actual.failure_count, expected.2, "case {case}");
        assert_eq!(actual.input_tokens, expected.3, "case {case}");
        assert_eq!(actual.output_tokens, expected.4, "case {case}");
        assert_eq!(actual.total_tokens, expected.5, "case {case}");
        assert_eq!(actual.estimated_cost_usd, Some(expected.6), "case {case}");
        assert_eq!(actual.total_latency_ms, expected.7, "case {case}");
        assert_eq!(
            actual.p95_latency_ms.map(f64::to_bits),
            expected.8.map(f64::to_bits),
            "case {case}"
        );
        assert_eq!(actual.fallback_count, expected.9, "case {case}");
        assert_eq!(actual.policy_denial_count, expected.10, "case {case}");
        assert_eq!(actual.rate_limit_denial_count, expected.11, "case {case}");
        assert_eq!(actual.budget_denial_count, expected.12, "case {case}");
        assert_eq!(actual.guardrail_block_count, 0, "case {case}");
        assert_eq!(actual.expensive_request_count, 0, "case {case}");
        assert_eq!(
            actual.average_latency_ms,
            (expected.0 > 0).then(|| expected.7 as f64 / expected.0 as f64),
            "case {case}"
        );
        // Explicit minimum cost also remains a row filter, rather than merely
        // changing the expensive-request counter in the grouped aggregate.
        query.min_cost_usd = Some(0.25);
        let filtered = f.store.usage_summary(query).await.unwrap();
        assert_eq!(filtered.request_count, expected.13, "case {case}");
        assert_eq!(filtered.expensive_request_count, expected.13, "case {case}");
    }
    f.cleanup().await;
}

#[tokio::test]
async fn usage_cursor_keeps_duplicate_ids_and_excludes_late_and_uncommitted_inserts() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let at = Utc::now() - Duration::hours(1);
    for tokens in 1..=5 {
        f.insert("reused-request-id", at, tokens).await;
    }
    let mut pending = f.store.pool().begin().await.unwrap();
    sqlx::query(INSERT_EVENT)
        .bind(Uuid::new_v4())
        .bind(f.key)
        .bind(f.project)
        .bind("pending")
        .bind(at - Duration::seconds(1))
        .bind(999_i64)
        .execute(&mut *pending)
        .await
        .unwrap();
    let mut query = f.query();
    query.limit = Some(2);
    query.cursor = Some("start".to_owned());
    let first = f.store.usage_events(query.clone()).await.unwrap();
    assert_eq!(first.rows.len(), 2);
    assert!(first.next_cursor.is_some());
    let mut snapshot_query = f.query();
    snapshot_query.cursor = first.snapshot_cursor.clone();
    pending.commit().await.unwrap();
    // Both ordinary new events and newly persisted old timestamps stay outside
    // the first page's visibility boundary.
    f.insert("late-same-time", at, 998).await;
    f.insert("late-backdated", at - Duration::minutes(1), 997)
        .await;
    let mut tokens = first
        .rows
        .iter()
        .map(|row| row.total_tokens)
        .collect::<Vec<_>>();
    let mut cursor = first.next_cursor;
    while let Some(next) = cursor {
        query.cursor = Some(next);
        let page = f.store.usage_events(query.clone()).await.unwrap();
        tokens.extend(page.rows.iter().map(|row| row.total_tokens));
        cursor = page.next_cursor;
    }
    tokens.sort_unstable();
    assert_eq!(tokens, [1, 2, 3, 4, 5]);
    let summary = f.store.usage_summary(snapshot_query).await.unwrap();
    assert_eq!(summary.request_count, 5);
    assert_eq!(summary.total_tokens, 15);
    // No dates and no cursor retain the released all-time/offset semantics.
    query.cursor = None;
    query.offset = Some(2);
    let legacy = f.store.usage_events(query).await.unwrap();
    assert_eq!(legacy.offset, 2);
    assert!(legacy.next_cursor.is_none());
    assert_eq!(
        f.store
            .usage_summary(f.query())
            .await
            .unwrap()
            .request_count,
        8
    );
    f.cleanup().await;
}

#[tokio::test]
async fn usage_export_cursor_matches_legacy_order_and_counts_only_scoped_guardrails() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let at = Utc::now() - Duration::days(40);
    for tokens in 1..=5 {
        f.insert(&format!("request-{tokens}"), at, tokens).await;
    }
    sqlx::query("INSERT INTO guardrail_execution_events (request_id,key_id,project_id,guardrail_name,mode,action,failure_policy,latency_ms) VALUES ('request-1',$1,$2,'test','pre_call','block','fail_closed',1),('request-1',NULL,$2,'test','pre_call','block','fail_closed',1)")
        .bind(f.key).bind(f.project).execute(f.store.pool()).await.unwrap();
    let mut query = f.query();
    query.limit = Some(2);
    query.cursor = Some("start".to_owned());
    let mut actual = Vec::new();
    loop {
        let page = f.store.usage_export_page(query.clone()).await.unwrap();
        actual.extend(page.rows);
        if !page.has_more {
            break;
        }
        query.cursor = page.next_cursor;
    }
    let mut legacy = f.query();
    legacy.limit = Some(5);
    let export = f.store.usage_export(legacy).await.unwrap();
    assert_eq!(actual, export.rows);
    assert_eq!(actual[0].guardrail_action_count, 1);
    assert_eq!(export.summary.guardrail_block_count, 1);
    assert_eq!(export.summary.expensive_request_count, 5);
    assert_eq!(export.summary.estimated_cost_usd, Some(6.25));
    match f
        .store
        .usage_section(f.query(), UsageSectionKind::Timeseries)
        .await
        .unwrap()
    {
        UsageSection::Timeseries { rows, page } => {
            assert_eq!(rows.len(), 1);
            assert_eq!(page.limit, 50);
        }
        _ => panic!("timeseries section shape"),
    }
    query.cursor = Some("malformed-cursor".to_owned());
    assert!(matches!(
        f.store.usage_events(query).await,
        Err(gateway_core::GatewayError::InvalidUsageQuery)
    ));
    f.cleanup().await;
}

#[tokio::test]
async fn usage_snapshot_keeps_full_epoch_and_legacy_visibility() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let at = Utc::now() - Duration::hours(1);
    // Exercise epoch one without advancing the cluster's real transaction ID.
    // The in-progress and later markers have the same low 32 bits as old XIDs.
    for (request, marker) in [
        ("legacy", None),
        ("before-xmin", Some("4294967346")),
        ("committed", Some("4294967416")),
        ("in-progress", Some("4294967446")),
        ("at-xmax", Some("4294967496")),
        ("later", Some("4294967546")),
    ] {
        f.insert(request, at, 1).await;
        sqlx::query("UPDATE usage_events SET ingestion_xid=$1::text::xid8 WHERE project_id=$2 AND request_id=$3")
            .bind(marker).bind(f.project).bind(request).execute(f.store.pool()).await.unwrap();
        sqlx::query(INSERT_GUARDRAIL)
            .bind(request)
            .bind(f.key)
            .bind(f.project)
            .execute(f.store.pool())
            .await
            .unwrap();
        sqlx::query("UPDATE guardrail_execution_events SET ingestion_xid=$1::text::xid8 WHERE project_id=$2 AND request_id=$3")
            .bind(marker).bind(f.project).bind(request).execute(f.store.pool()).await.unwrap();
    }
    let mut query = f.query();
    query.cursor = Some(
        serde_json::json!({
            "v": 1, "snapshot": "4294967396:4294967496:4294967446",
            "descending": false, "after": null,
        })
        .to_string(),
    );
    let export = f.store.usage_export(query.clone()).await.unwrap();
    assert_eq!(
        export
            .rows
            .iter()
            .map(|row| row.request_id.as_str())
            .collect::<Vec<_>>(),
        ["before-xmin", "committed", "legacy"]
    );
    assert!(export
        .rows
        .iter()
        .all(|row| row.guardrail_action_count == 1));
    assert_eq!(export.summary.request_count, 3);
    assert_eq!(export.summary.guardrail_block_count, 3);
    query.sort_order = Some("asc".to_owned());
    assert_eq!(f.store.usage_events(query).await.unwrap().rows, export.rows);
    f.cleanup().await;
}

#[tokio::test]
async fn repricing_between_pages_preserves_every_insertion_and_export_summary() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let name = format!("paging-{}", Uuid::new_v4().simple());
    f.store
        .create_service(
            serde_json::from_value(serde_json::json!({
                "name": name, "upstream_base_url": "http://pricing.internal",
                "cost_mode": "fixed", "estimated_cost_usd": 1.25,
            }))
            .unwrap(),
        )
        .await
        .unwrap();
    let at = Utc::now() - Duration::hours(1);
    for number in 1..=5 {
        f.insert(
            &format!("request-{number}"),
            at + Duration::seconds(number),
            number,
        )
        .await;
    }
    sqlx::query("UPDATE usage_events SET service_name=$1, cost_mode='fixed', cost_source='service_default_fixed' WHERE project_id=$2")
        .bind(&name).bind(f.project).execute(f.store.pool()).await.unwrap();
    let identities_before: Vec<(Uuid, Option<String>, String)> = sqlx::query_as(
        "SELECT id, ingestion_xid::text, xmin::text FROM usage_events WHERE project_id=$1 ORDER BY id",
    ).bind(f.project).fetch_all(f.store.pool()).await.unwrap();
    assert!(identities_before
        .iter()
        .all(|(_, marker, _)| marker.is_some()));
    let mut query = f.query();
    query.limit = Some(2);
    query.cursor = Some("start".to_owned());
    let first_recent = f.store.usage_events(query.clone()).await.unwrap();
    let first_export = f.store.usage_export_page(query.clone()).await.unwrap();
    let repriced = f
        .store
        .patch_service(
            &name,
            serde_json::from_value(serde_json::json!({
                "estimated_cost_usd": 2.5, "reprice_existing_usage": true,
            }))
            .unwrap(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        repriced
            .historical_usage_repricing
            .unwrap()
            .updated_requests,
        5
    );
    let identities_after: Vec<(Uuid, Option<String>, String)> = sqlx::query_as(
        "SELECT id, ingestion_xid::text, xmin::text FROM usage_events WHERE project_id=$1 ORDER BY id",
    ).bind(f.project).fetch_all(f.store.pool()).await.unwrap();
    for (before, after) in identities_before.iter().zip(&identities_after) {
        assert_eq!((&before.0, &before.1), (&after.0, &after.1));
        assert_ne!(
            before.2, after.2,
            "repricing must actually create a new row version"
        );
    }
    for (export, first) in [(false, first_recent), (true, first_export)] {
        let snapshot = first.snapshot_cursor.clone();
        let mut actual = first.rows;
        let mut cursor = first.next_cursor;
        while let Some(next) = cursor {
            query.cursor = Some(next);
            let page = if export {
                f.store.usage_export_page(query.clone()).await.unwrap()
            } else {
                f.store.usage_events(query.clone()).await.unwrap()
            };
            assert!(page
                .rows
                .iter()
                .all(|row| row.estimated_cost_usd == Some(2.5)));
            actual.extend(page.rows);
            cursor = page.next_cursor;
        }
        let expected = if export {
            vec![1, 2, 3, 4, 5]
        } else {
            vec![5, 4, 3, 2, 1]
        };
        assert_eq!(
            actual
                .iter()
                .map(|row| row.total_tokens)
                .collect::<Vec<_>>(),
            expected
        );
        let mut snapshot_query = f.query();
        snapshot_query.cursor = snapshot;
        let summary = f.store.usage_summary(snapshot_query).await.unwrap();
        assert_eq!(
            summary.request_count, 5,
            "already emitted rows must remain in the summary"
        );
        assert_eq!(summary.estimated_cost_usd, Some(12.5));
    }
    sqlx::query("DELETE FROM service_registrations WHERE name=$1")
        .bind(name)
        .execute(f.store.pool())
        .await
        .unwrap();
    f.cleanup().await;
}

#[tokio::test]
async fn late_guardrail_commits_do_not_change_snapshot_rows_or_summary() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let at = Utc::now() - Duration::hours(1);
    f.insert("request-a", at, 1).await;
    f.insert("request-b", at, 1).await;
    sqlx::query(INSERT_GUARDRAIL)
        .bind("request-a")
        .bind(f.key)
        .bind(f.project)
        .execute(f.store.pool())
        .await
        .unwrap();
    let mut pending = f.store.pool().begin().await.unwrap();
    sqlx::query(INSERT_GUARDRAIL)
        .bind("request-b")
        .bind(f.key)
        .bind(f.project)
        .execute(&mut *pending)
        .await
        .unwrap();
    let mut query = f.query();
    query.limit = Some(1);
    query.cursor = Some("start".to_owned());
    let first = f.store.usage_export_page(query.clone()).await.unwrap();
    assert_eq!(first.rows[0].guardrail_action_count, 1);
    pending.commit().await.unwrap();
    sqlx::query(INSERT_GUARDRAIL)
        .bind("request-b")
        .bind(f.key)
        .bind(f.project)
        .execute(f.store.pool())
        .await
        .unwrap();
    query.cursor = first.next_cursor;
    let second = f.store.usage_export_page(query).await.unwrap();
    assert_eq!(second.rows[0].request_id, "request-b");
    assert_eq!(second.rows[0].guardrail_action_count, 0);
    let mut snapshot_query = f.query();
    snapshot_query.cursor = first.snapshot_cursor;
    let snapshot = f.store.usage_export(snapshot_query).await.unwrap();
    assert_eq!(snapshot.summary.guardrail_block_count, 1);
    assert_eq!(
        snapshot
            .rows
            .iter()
            .map(|row| row.guardrail_action_count)
            .sum::<i64>(),
        1
    );
    let current = f.store.usage_export(f.query()).await.unwrap();
    assert_eq!(current.summary.guardrail_block_count, 2);
    assert_eq!(current.rows[1].guardrail_action_count, 2);
    f.cleanup().await;
}

#[tokio::test]
async fn insertion_visibility_migration_preserves_legacy_rows_without_rewrite() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let mut tx = f.store.pool().begin().await.unwrap();
    // Shadow both real tables on one connection to test the exact migration
    // against pre-migration rows without altering the shared integration schema.
    sqlx::raw_sql("CREATE TEMP TABLE usage_events (id integer); CREATE TEMP TABLE guardrail_execution_events (id integer); INSERT INTO usage_events VALUES (1); INSERT INTO guardrail_execution_events VALUES (1);")
        .execute(&mut *tx).await.unwrap();
    let before: Vec<(String, i64)> = sqlx::query_as("SELECT relname::text, relfilenode::bigint FROM pg_class WHERE oid IN ('pg_temp.usage_events'::regclass, 'pg_temp.guardrail_execution_events'::regclass) ORDER BY relname")
        .fetch_all(&mut *tx).await.unwrap();
    sqlx::raw_sql(include_str!(
        "../migrations/20261023000300_usage_insertion_visibility.sql"
    ))
    .execute(&mut *tx)
    .await
    .unwrap();
    let after: Vec<(String, i64)> = sqlx::query_as("SELECT relname::text, relfilenode::bigint FROM pg_class WHERE oid IN ('pg_temp.usage_events'::regclass, 'pg_temp.guardrail_execution_events'::regclass) ORDER BY relname")
        .fetch_all(&mut *tx).await.unwrap();
    assert_eq!(
        before, after,
        "adding the marker must not rewrite either ledger"
    );
    for table in ["usage_events", "guardrail_execution_events"] {
        let marker: Option<String> = sqlx::query_scalar(&format!(
            "SELECT ingestion_xid::text FROM {table} WHERE id=1"
        ))
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        assert_eq!(marker, None, "historical rows remain NULL");
        let marker: String = sqlx::query_scalar(&format!(
            "INSERT INTO {table} (id) VALUES (2) RETURNING ingestion_xid::text"
        ))
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        let xid: String = sqlx::query_scalar("SELECT pg_current_xact_id()::text")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        assert_eq!(
            marker, xid,
            "old explicit-column writers get the insertion default"
        );
    }
    tx.rollback().await.unwrap();
    f.cleanup().await;
}
