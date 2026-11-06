use gateway_core::{GatewayError, OperatorTokenMaterial, OperatorTokenStore};
use gateway_store::PostgresStore;
use sqlx::postgres::PgPoolOptions;
use std::time::Duration;
use uuid::Uuid;

#[tokio::test]
async fn concurrent_operator_bootstrap_keeps_the_winner_without_restarting() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("skipping integration test: DATABASE_URL is not set");
        return;
    };
    let admin = PgPoolOptions::new().connect(&database_url).await.unwrap();
    let schema = format!("operator_bootstrap_{}", Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&admin)
        .await
        .unwrap();
    let search_path = format!("SET search_path TO {schema}");
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .after_connect(move |connection, _| {
            let sql = search_path.clone();
            Box::pin(async move {
                sqlx::query(&sql).execute(connection).await?;
                Ok(())
            })
        })
        .connect(&database_url)
        .await
        .unwrap();
    sqlx::raw_sql(include_str!(
        "../migrations/20260510000200_phase_7_operator_tokens.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    sqlx::raw_sql(include_str!(
        "../migrations/20260522000100_operator_scopes_audit_events.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    let backend: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&pool)
        .await
        .unwrap();
    let store = PostgresStore::new(pool);
    let configured = OperatorTokenMaterial::generate().unwrap();
    for same_prefix in [true, false] {
        let contender = if same_prefix {
            configured.clone()
        } else {
            OperatorTokenMaterial::generate().unwrap()
        };
        // The winner remains uncommitted so the contender's snapshot sees no
        // active token, then waits on the conflicting unique index entry.
        let mut winner = admin.begin().await.unwrap();
        sqlx::query(&format!(
            "INSERT INTO {schema}.operator_tokens (token_prefix, token_hash) VALUES ($1, $2)"
        ))
        .bind(&configured.token_prefix)
        .bind(&configured.token_hash)
        .execute(&mut *winner)
        .await
        .unwrap();
        let competing_store = store.clone();
        let task =
            tokio::spawn(async move { competing_store.bootstrap_operator_token(&contender).await });
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let waiting: bool = sqlx::query_scalar(
                    "SELECT EXISTS (SELECT 1 FROM pg_stat_activity WHERE pid = $1 AND wait_event_type = 'Lock')",
                )
                .bind(backend)
                .fetch_one(&admin)
                .await
                .unwrap();
                if waiting {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("contender must reach the database conflict before winner commits");
        winner.commit().await.unwrap();
        assert!(task.await.unwrap().unwrap().is_none());
        let rows: Vec<(String, String)> =
            sqlx::query_as("SELECT token_prefix, token_hash FROM operator_tokens")
                .fetch_all(store.pool())
                .await
                .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, configured.token_prefix);
        assert_eq!(rows[0].1, configured.token_hash);
        sqlx::query("DELETE FROM operator_tokens")
            .execute(store.pool())
            .await
            .unwrap();
    }
    // A historical disabled prefix is not a successful bootstrap race.
    sqlx::query(
        "INSERT INTO operator_tokens (token_prefix, token_hash, disabled) VALUES ($1, $2, true)",
    )
    .bind(&configured.token_prefix)
    .bind(&configured.token_hash)
    .execute(store.pool())
    .await
    .unwrap();
    assert_eq!(
        store
            .bootstrap_operator_token(&configured)
            .await
            .unwrap_err(),
        GatewayError::StoreUnavailable
    );
    store.pool().close().await;
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}
