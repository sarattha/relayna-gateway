//! Full proxy/control-plane regressions. Requires DATABASE_URL and REDIS_URL;
//! uses an isolated PostgreSQL database (test role needs CREATEDB) and unique Redis keys, never shared data.
use axum::{body::Body, http::StatusCode, response::IntoResponse, routing::post, Json, Router};
use gateway_core::{
    traffic::{TrafficQuery, TrafficStore},
    AdminKeyStore, OperatorTokenMaterial, OperatorTokenStore,
};
use gateway_store::PostgresStore;
use serde_json::{json, Value};
use std::{
    process::{Child, Command, Stdio},
    time::Duration,
};
use uuid::Uuid;

struct GatewayProcess(Child);
impl Drop for GatewayProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
struct TestDatabase {
    url: String,
    name: String,
}
impl Drop for TestDatabase {
    fn drop(&mut self) {
        let url = self.url.clone();
        let name = self.name.clone();
        // Cleanup also runs on assertion failure after the gateway child is killed.
        let _ = std::thread::spawn(move || {
            let runtime = tokio::runtime::Runtime::new().unwrap();
            runtime.block_on(async {
                if let Ok(pool) = sqlx::PgPool::connect(&url).await {
                    let _ = sqlx::query(&format!("DROP DATABASE {name} WITH (FORCE)"))
                        .execute(&pool)
                        .await;
                }
            });
        })
        .join();
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

async fn upstream(Json(body): Json<Value>) -> axum::response::Response {
    if body["model"] == "fail" {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"upstream": "busy"})),
        )
            .into_response();
    }
    if body["model"] == "stream" {
        let stream = futures_util::stream::unfold(0, |step| async move {
            match step {
                0 => Some((Ok::<_, std::io::Error>("data: {\"text\":\"hello\"}\n\n"), 1)),
                1 => {
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    Some((
                        Err(std::io::Error::other("synthetic upstream interruption")),
                        2,
                    ))
                }
                _ => None,
            }
        });
        return (
            [("content-type", "text/event-stream")],
            Body::from_stream(stream),
        )
            .into_response();
    }
    Json(json!({"ok":true})).into_response()
}

async fn saved(store: &PostgresStore, id: &str) -> gateway_core::traffic::TrafficRequest {
    for _ in 0..80 {
        let rows = store
            .traffic_history(TrafficQuery {
                request_id: Some(id.into()),
                ..Default::default()
            })
            .await
            .unwrap();
        if let Some(row) = rows.into_iter().next() {
            return row;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("missing diagnostic {id}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_proxy_captures_early_failures_attempts_stream_abort_and_recording_errors() {
    let (Ok(database), Ok(redis_url)) = (std::env::var("DATABASE_URL"), std::env::var("REDIS_URL"))
    else {
        eprintln!("skipping traffic integration: DATABASE_URL and REDIS_URL are required");
        return;
    };
    let admin_pool = sqlx::PgPool::connect(&database).await.unwrap();
    let schema = format!("traffic_{}", Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE DATABASE {schema}"))
        .execute(&admin_pool)
        .await
        .unwrap();
    let _database_cleanup = TestDatabase {
        url: database.clone(),
        name: schema.clone(),
    };
    let mut database_url = url::Url::parse(&database).unwrap();
    database_url.set_path(&format!("/{schema}"));
    let store = PostgresStore::connect(database_url.as_str()).await.unwrap();
    let operator = OperatorTokenMaterial::generate().unwrap();
    store.bootstrap_operator_token(&operator).await.unwrap();
    let key = gateway_core::VirtualKeyMaterial::generate().unwrap();
    let key_record = store.create_admin_key(serde_json::from_value(json!({
        "owner_type":"individual", "expires_at":null, "preset":"developer",
        "policy":{"allowed_routes":["/v1/chat/completions"], "allowed_models":[], "allow_streaming":true}
    })).unwrap(), &key).await.unwrap();
    let upstream_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_url = format!("http://{}", upstream_listener.local_addr().unwrap());
    let upstream_task = tokio::spawn(async move {
        axum::serve(
            upstream_listener,
            Router::new().route("/v1/chat/completions", post(upstream)),
        )
        .await
        .unwrap();
    });
    let proxy_port = free_port();
    let control_port = free_port();
    let log_path = std::env::temp_dir().join(format!("{schema}.log"));
    let log = std::fs::File::create(&log_path).unwrap();
    let mut process = GatewayProcess(
        Command::new(env!("CARGO_BIN_EXE_gateway-api"))
            .env("DATABASE_URL", database_url.as_str())
            .env("REDIS_URL", &redis_url)
            .env("GATEWAY_BIND_ADDR", format!("127.0.0.1:{proxy_port}"))
            .env(
                "GATEWAY_CONTROL_BIND_ADDR",
                format!("127.0.0.1:{control_port}"),
            )
            .env("LITELLM_BASE_URL", upstream_url)
            .env("LITELLM_SERVICE_KEY", "traffic-test-provider-credential")
            .env("ENTRA_AUTH_ENABLED", "false")
            .env("APIGEE_TRUSTED_HEADER_ENABLED", "false")
            .env("GATEWAY_MAX_INFLIGHT_BUFFER_BYTES", "128")
            .env("LOG_LEVEL", "info")
            .stdout(Stdio::from(log.try_clone().unwrap()))
            .stderr(Stdio::from(log))
            .spawn()
            .unwrap(),
    );
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        .build()
        .unwrap();
    let control = format!("http://127.0.0.1:{control_port}");
    let proxy = format!("http://127.0.0.1:{proxy_port}");
    let mut ready = false;
    for _ in 0..100 {
        if client
            .get(format!("{control}/admin-ui/healthz"))
            .send()
            .await
            .is_ok()
        {
            ready = true;
            break;
        }
        if process.0.try_wait().unwrap().is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        ready,
        "gateway did not start: {}",
        std::fs::read_to_string(&log_path).unwrap()
    );
    for path in ["live", "history"] {
        assert_eq!(
            client
                .get(format!("{control}/admin-ui/admin/traffic/{path}"))
                .send()
                .await
                .unwrap()
                .status(),
            401
        );
    }
    let response = client
        .post(format!("{proxy}/v1/chat/completions?token=hidden-query"))
        .header("x-request-id", "missing-auth")
        .body("hidden-request-body")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 401);
    assert_eq!(response.headers()["x-request-id"], "missing-auth");
    let row = saved(&store, "missing-auth").await;
    assert!(row.key_id.is_none());
    assert_eq!(row.attempts, 0);
    assert_eq!(row.timeline[0].stage, "received");
    assert_eq!(
        row.diagnostics.failure_stage.as_deref(),
        Some("authentication")
    );
    assert!(!serde_json::to_string(&row).unwrap().contains("hidden-"));
    let response = client
        .get(format!("{proxy}/unknown/secret-path"))
        .header("x-request-id", "unknown-route")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 404);
    let row = saved(&store, "unknown-route").await;
    assert!(row.endpoint.is_none());
    assert!(row.key_id.is_none());
    assert_eq!(row.diagnostics.failure_stage.as_deref(), Some("routing"));
    for (id, model, expected) in [
        ("success", "ok", 200),
        ("upstream-503", "fail", 503),
        ("stream-abort", "stream", 200),
    ] {
        let response = client
            .post(format!("{proxy}/v1/chat/completions"))
            .bearer_auth(&key.raw_key)
            .header("x-request-id", id)
            .json(&json!({"model":model, "stream":model=="stream", "messages":[]}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), expected, "{id}");
        if id == "upstream-503" {
            assert_eq!(
                response.json::<Value>().await.unwrap(),
                json!({"upstream":"busy"})
            );
        } else {
            let _ = response.bytes().await;
        }
        let row = saved(&store, id).await;
        assert_eq!(row.attempts, 1);
        assert_eq!(row.client_status, Some(expected));
        assert!(row
            .timeline
            .iter()
            .any(|step| step.stage == "upstream_connected"));
        if id == "upstream-503" {
            assert_eq!(row.diagnostics.failure_source.as_deref(), Some("upstream"));
        }
        if id == "stream-abort" {
            assert_eq!(
                row.diagnostics.outcome.as_deref(),
                Some("stream_interrupted")
            );
            let (status, diagnostics): (String, sqlx::types::Json<Value>) = sqlx::query_as(
                "SELECT status, diagnostics FROM usage_events WHERE request_id = 'stream-abort'",
            )
            .fetch_one(store.pool())
            .await
            .unwrap();
            assert_eq!(status, "failure");
            assert_eq!(diagnostics.0["outcome"], "stream_interrupted");
        }
    }
    let response = client
        .post(format!("{proxy}/v1/chat/completions"))
        .bearer_auth(&key.raw_key)
        .header("x-request-id", "body-capacity")
        .json(&json!({"model":"ok", "messages":["x".repeat(512)]}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 503);
    let _ = response.bytes().await;
    let row = saved(&store, "body-capacity").await;
    assert_eq!(
        row.diagnostics.failure_code.as_deref(),
        Some("gateway_overloaded")
    );
    assert_eq!(
        row.diagnostics.failure_stage.as_deref(),
        Some("body_admission")
    );
    // Corrupt only this test key's rate counter to force a real Redis operation
    // failure without stopping a shared dependency or affecting other keys.
    let mut redis = redis::Client::open(redis_url.as_str())
        .unwrap()
        .get_multiplexed_async_connection()
        .await
        .unwrap();
    let rate_key =
        gateway_core::rate_limits::request_rate_limit_key(key_record.id, chrono::Utc::now());
    redis::cmd("SET")
        .arg(&rate_key)
        .arg("not-an-integer")
        .arg("EX")
        .arg(120)
        .query_async::<()>(&mut redis)
        .await
        .unwrap();
    let response = client
        .post(format!("{proxy}/v1/chat/completions"))
        .bearer_auth(&key.raw_key)
        .header("x-request-id", "control-state-failure")
        .json(&json!({"model":"ok"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 503);
    let _ = response.bytes().await;
    let row = saved(&store, "control-state-failure").await;
    assert_eq!(row.attempts, 0);
    assert_eq!(
        row.diagnostics.failure_code.as_deref(),
        Some("control_state_unavailable")
    );
    assert_eq!(row.diagnostics.failure_stage.as_deref(), Some("rate_limit"));
    redis::cmd("DEL")
        .arg(&rate_key)
        .query_async::<()>(&mut redis)
        .await
        .unwrap();
    let history = client.get(format!("{control}/admin-ui/admin/traffic/history?status=503&failure_code=control_state_unavailable"))
        .bearer_auth(&operator.raw_token).send().await.unwrap().json::<Value>().await.unwrap();
    assert_eq!(history.as_array().unwrap().len(), 1);
    assert_eq!(
        client
            .get(format!(
                "{control}/admin-ui/admin/traffic/history?limit=201"
            ))
            .bearer_auth(&operator.raw_token)
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    // Every diagnostic destination fails independently; observe live evidence and logs.
    for table in ["usage_events", "request_debug_bundles", "request_traffic"] {
        sqlx::query(&format!("ALTER TABLE {table} ADD CONSTRAINT test_recording_failure CHECK (request_id <> 'recording-failure') NOT VALID"))
            .execute(store.pool()).await.unwrap();
    }
    let _ = client
        .post(format!("{proxy}/v1/chat/completions"))
        .bearer_auth(&key.raw_key)
        .header("x-request-id", "recording-failure")
        .json(&json!({"model":"fail"}))
        .send()
        .await
        .unwrap()
        .bytes()
        .await;
    tokio::time::sleep(Duration::from_millis(400)).await;
    let mut live = client
        .get(format!("{control}/admin-ui/admin/traffic/live"))
        .bearer_auth(&operator.raw_token)
        .header("last-event-id", "previous-instance:1")
        .send()
        .await
        .unwrap();
    assert_eq!(live.headers()["cache-control"], "private, no-store");
    let mut text = String::new();
    while !text.contains("\n\n") {
        text.push_str(&String::from_utf8_lossy(
            &live.chunk().await.unwrap().unwrap(),
        ));
        assert!(text.len() < 512 * 1024);
    }
    let batch: Value = serde_json::from_str(
        text.lines()
            .find_map(|line| line.strip_prefix("data: "))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(batch["gap"], true);
    let failed = batch["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["request_id"] == "recording-failure")
        .unwrap();
    for destination in ["usage_events", "debug_bundles", "request_traffic"] {
        assert!(failed["recording_failures"]
            .as_array()
            .unwrap()
            .contains(&json!(destination)));
    }
    assert!(!text.contains("traffic-test-provider-credential"));
    let logs = std::fs::read_to_string(&log_path).unwrap();
    assert!(logs.contains("gateway diagnostic recording failed"));
    assert!(logs.contains("recording-failure"));
    drop(live);
    drop(process);
    upstream_task.abort();
    store.pool().close().await;
}
