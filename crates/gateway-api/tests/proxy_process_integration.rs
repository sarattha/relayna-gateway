use axum::{
    body::Bytes,
    extract::OriginalUri,
    http::StatusCode,
    routing::{any, post},
    Json, Router,
};
use gateway_api::app;
use gateway_core::{
    admin::KeyPolicyPatch, AdminKeyCreate, AdminKeyOwnerType, AdminKeyStore, AdminPolicyLayerStore,
    AdminPolicyLayerUpsert, AdminProjectStore, AdminServiceStore, GuardrailPolicy, PolicyLayerKind,
    ProjectCreateRequest, ServiceCreateRequest, VirtualKeyMaterial,
};
use gateway_proxy::{PingoraLiteLlmConfig, PingoraUpstreamConfig, RelaynaPingoraProxy};
use gateway_store::{PostgresStore, RedisControlState, RedisReadiness};
use pingora_core::server::Server;
use serde_json::{json, Value};
use std::{net::TcpListener as StdTcpListener, sync::Arc, time::Duration};
use tokio::{net::TcpListener, task::JoinHandle, time};
use uuid::Uuid;

fn unused_port() -> u16 {
    let listener = StdTcpListener::bind("127.0.0.1:0").expect("reserve local port");
    listener.local_addr().expect("local address").port()
}

async fn mock_upstream() -> (String, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock upstream");
    let address = listener.local_addr().expect("mock upstream address");
    let app = Router::new()
        .route("/stream", post(|body: Bytes| async move { body }))
        .route(
            "/large-response",
            post(|| async { Json(json!({"payload": "x".repeat(2048)})) }),
        )
        .fallback(any(|OriginalUri(uri): OriginalUri| async move {
            (
                StatusCode::OK,
                Json(json!({
                    "id": "mock-response",
                    "path": uri.path(),
                    "model": "coverage-model",
                    "choices": [],
                    "usage": {"prompt_tokens": 2, "completion_tokens": 3, "total_tokens": 5},
                    "usage_metadata": {"total_cost": 0.001}
                })),
            )
        }));
    let task = tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve mock upstream");
    });
    (format!("http://{address}"), task)
}

async fn wait_until_ready(client: &reqwest::Client, control_url: &str) {
    for _ in 0..100 {
        if client
            .get(format!("{control_url}/admin-ui/readyz"))
            .send()
            .await
            .is_ok_and(|response| response.status().is_success())
        {
            return;
        }
        time::sleep(Duration::from_millis(100)).await;
    }
    panic!("gateway did not become ready");
}

async fn send_json(
    client: &reqwest::Client,
    proxy_url: &str,
    path: &str,
    key: Option<&str>,
    body: Value,
) -> reqwest::Response {
    let mut request = client.post(format!("{proxy_url}{path}")).json(&body);
    if let Some(key) = key {
        request = request.bearer_auth(key);
    }
    request.send().await.expect("proxy response")
}

#[tokio::test]
async fn gateway_process_proxies_generation_direct_and_registered_service_routes() {
    let database_url = match std::env::var("DATABASE_URL") {
        Ok(value) => value,
        Err(_) => {
            eprintln!("skipping process proxy coverage: DATABASE_URL is not set");
            return;
        }
    };
    let redis_url = match std::env::var("REDIS_URL") {
        Ok(value) => value,
        Err(_) => {
            eprintln!("skipping process proxy coverage: REDIS_URL is not set");
            return;
        }
    };
    let store = PostgresStore::connect(&database_url)
        .await
        .expect("connect test store");
    let mut database_lock = store
        .pool()
        .acquire()
        .await
        .expect("acquire integration lock");
    sqlx::query("SELECT pg_advisory_lock(82120260808)")
        .execute(&mut *database_lock)
        .await
        .expect("serialize shared control-plane integration state");
    let (upstream_url, upstream_task) = mock_upstream().await;
    sqlx::query(
        r#"
        UPDATE provider_configs
        SET base_url = $1,
            credential_secret = 'litellm-secret',
            credential_header_mode = 'authorization_bearer',
            credential_header_name = NULL,
            credential_header_value_format = 'raw'
        WHERE provider = 'litellm' AND enabled = true
        "#,
    )
    .bind(&upstream_url)
    .execute(store.pool())
    .await
    .expect("point persisted LiteLLM config at mock upstream");
    sqlx::query(
        r#"
        UPDATE openai_route_settings
        SET mode = 'managed_by_gateway',
            updated_at = now()
        WHERE route_id IN ('chat-completions', 'responses')
        "#,
    )
    .execute(store.pool())
    .await
    .expect("establish managed routes for virtual-key proxy coverage");
    let suffix = Uuid::new_v4().simple().to_string();
    let project = store
        .create_project(ProjectCreateRequest {
            name: format!("proxy-coverage-{suffix}"),
        })
        .await
        .expect("create proxy project");
    store
        .upsert_policy_layer(AdminPolicyLayerUpsert {
            kind: PolicyLayerKind::Project,
            scope_id: Some(project.id.to_string()),
            policy: KeyPolicyPatch::default(),
            guardrail_policy: Default::default(),
        })
        .await
        .expect("create neutral project policy layer");
    let service_name = format!("proxy-coverage-{suffix}");
    store
        .create_service(
            serde_json::from_value::<ServiceCreateRequest>(json!({
                "name": service_name,
                "project_id": project.id,
                "route_pattern": format!("/services/{service_name}/*"),
                "upstream_base_url": upstream_url,
                "credential": "service-secret",
                "allowed_methods": ["GET", "POST"],
                "cost_mode": "fixed",
                "estimated_cost_usd": 0.01,
                "pricing_rules": [{
                    "name": "docint",
                    "json_pointer": "/engine",
                    "equals": "docint",
                    "cost_mode": "fixed",
                    "estimated_cost_usd": 0.5
                }]
            }))
            .expect("service request"),
        )
        .await
        .expect("create proxy service");
    let material = VirtualKeyMaterial::generate().expect("virtual key");
    store
        .create_admin_key(
            AdminKeyCreate {
                owner_type: AdminKeyOwnerType::Project,
                project_id: Some(project.id),
                service_names: vec![service_name.clone()],
                preset: None,
                expires_at: None,
                rotation_due_at: None,
                policy: KeyPolicyPatch {
                    allowed_routes: Some(vec![
                        "/v1/chat/completions".to_owned(),
                        "/v1/responses".to_owned(),
                        "/v1/embeddings".to_owned(),
                        "/v1/rerank".to_owned(),
                        "/v1/messages".to_owned(),
                        "/providers/openai/*".to_owned(),
                        "/services/*".to_owned(),
                    ]),
                    allowed_providers: Some(vec![
                        "litellm".to_owned(),
                        "openai-compatible".to_owned(),
                        "internal-service".to_owned(),
                    ]),
                    allowed_services: Some(vec![service_name.clone()]),
                    allow_streaming: Some(true),
                    allow_tools: Some(true),
                    ..KeyPolicyPatch::default()
                },
                guardrail_policy: GuardrailPolicy {
                    mandatory_guardrails: vec!["pii-redact".to_owned()],
                    ..GuardrailPolicy::default()
                },
            },
            &material,
        )
        .await
        .expect("create proxy key");

    let stream_service_name = format!("proxy-stream-{suffix}");
    store
        .create_service(
            serde_json::from_value::<ServiceCreateRequest>(json!({
                "name": stream_service_name,
                "project_id": project.id,
                "route_pattern": format!("/services/{stream_service_name}/*"),
                "upstream_base_url": upstream_url,
                "credential": "stream-service-secret",
                "allowed_methods": ["POST"],
                "cost_mode": "fixed",
                "estimated_cost_usd": 0.01,
                "pricing_rules": []
            }))
            .expect("streaming service request"),
        )
        .await
        .expect("create streaming proxy service");
    let stream_material = VirtualKeyMaterial::generate().expect("stream virtual key");
    store
        .create_admin_key(
            AdminKeyCreate {
                owner_type: AdminKeyOwnerType::Project,
                project_id: Some(project.id),
                service_names: vec![stream_service_name.clone()],
                preset: None,
                expires_at: None,
                rotation_due_at: None,
                policy: KeyPolicyPatch {
                    allowed_routes: Some(vec!["/services/*".to_owned()]),
                    allowed_providers: Some(vec!["internal-service".to_owned()]),
                    allowed_services: Some(vec![stream_service_name.clone()]),
                    ..KeyPolicyPatch::default()
                },
                guardrail_policy: GuardrailPolicy::default(),
            },
            &stream_material,
        )
        .await
        .expect("create streaming proxy key");

    let redis = RedisReadiness::new(&redis_url).expect("redis readiness");
    let redis_control = RedisControlState::new(&redis_url).expect("redis control state");
    let control_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind control listener");
    let control_port = control_listener
        .local_addr()
        .expect("control address")
        .port();
    let control_app = app::router(store.clone(), redis);
    let control_task = tokio::spawn(async move {
        axum::serve(control_listener, control_app)
            .await
            .expect("serve control API");
    });

    let proxy_port = unused_port();
    let proxy_config = PingoraLiteLlmConfig::from_base_url(&upstream_url, "litellm-secret")
        .expect("LiteLLM proxy config")
        .with_direct_openai(Some(
            PingoraUpstreamConfig::from_base_url(&upstream_url, "openai-secret")
                .expect("direct OpenAI config"),
        ))
        .with_worker_token(Some("worker-secret".to_owned()))
        .with_body_admission_limits(2, 512)
        .expect("body admission limits");
    let proxy = RelaynaPingoraProxy::new(Arc::new(store), Arc::new(redis_control), proxy_config);
    std::thread::spawn(move || {
        let mut pingora = Server::new(None).expect("create Pingora server");
        pingora.bootstrap();
        let mut service = pingora_proxy::http_proxy_service(&pingora.configuration, proxy);
        service.add_tcp(&format!("127.0.0.1:{proxy_port}"));
        pingora.add_service(service);
        pingora.run_forever();
    });
    let client = reqwest::Client::new();
    let proxy_url = format!("http://127.0.0.1:{proxy_port}");
    let control_url = format!("http://127.0.0.1:{control_port}");
    wait_until_ready(&client, &control_url).await;

    assert_eq!(
        send_json(
            &client,
            &proxy_url,
            "/v1/chat/completions",
            None,
            json!({"model": "coverage-model", "messages": []}),
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        send_json(
            &client,
            &proxy_url,
            "/v1/chat/completions",
            Some("malformed"),
            json!({"model": "coverage-model", "messages": []}),
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );

    for (path, body) in [
        (
            "/v1/chat/completions",
            json!({"model": "coverage-model", "messages": [{"role": "user", "content": "hello"}]}),
        ),
        (
            "/chat/completions",
            json!({"model": "coverage-model", "messages": [{"role": "user", "content": "hello"}]}),
        ),
        (
            "/v1/responses",
            json!({"model": "coverage-model", "input": "hello"}),
        ),
        (
            "/responses",
            json!({"model": "coverage-model", "input": "hello"}),
        ),
        (
            "/v1/embeddings",
            json!({"model": "coverage-model", "input": "hello"}),
        ),
        (
            "/rerank",
            json!({"model": "coverage-model", "query": "hello", "documents": ["one", "two"]}),
        ),
        (
            "/v1/rerank",
            json!({"model": "coverage-model", "query": "hello", "documents": ["one", "two"]}),
        ),
        (
            "/v2/rerank",
            json!({"model": "coverage-model", "query": "hello", "documents": ["one", "two"]}),
        ),
        (
            "/v1/messages",
            json!({"model": "coverage-model", "messages": [{"role": "user", "content": "hello"}], "max_tokens": 8}),
        ),
        (
            "/providers/openai/v1/chat/completions",
            json!({"model": "coverage-model", "messages": []}),
        ),
        (
            &format!("/services/{service_name}/run"),
            json!({"engine": "docint", "payload": "hello"}),
        ),
    ] {
        let response = send_json(&client, &proxy_url, path, Some(&material.raw_key), body).await;
        let status = response.status();
        let response_body: Value = response.json().await.expect("proxy response body");
        assert_eq!(status, StatusCode::OK, "proxy path {path}: {response_body}");
        if matches!(path, "/chat/completions" | "/responses") || path.ends_with("/rerank") {
            assert_eq!(response_body["path"], path, "preserve alias path {path}");
        }
    }

    let service_url = format!("{proxy_url}/services/{service_name}/status");
    let response = client
        .get(&service_url)
        .bearer_auth(&material.raw_key)
        .send()
        .await
        .expect("GET service response");
    assert_eq!(response.status(), StatusCode::OK);

    let boundary = "proxy-coverage-boundary";
    let multipart = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"engine\"\r\n\r\ndocint\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"coverage.txt\"\r\nContent-Type: text/plain\r\n\r\ncoverage document\r\n--{boundary}--\r\n"
    );
    let response = client
        .post(format!("{proxy_url}/services/{service_name}/run"))
        .bearer_auth(&material.raw_key)
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(multipart)
        .send()
        .await
        .expect("multipart service response");
    assert_eq!(response.status(), StatusCode::OK);

    let response = send_json(
        &client,
        &proxy_url,
        "/v1/chat/completions",
        Some(&material.raw_key),
        json!({
            "model": "coverage-model",
            "stream": true,
            "messages": [{"role": "user", "content": "email alice@example.com"}]
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let _ = response.bytes().await.expect("consume streaming response");

    let opaque_body = vec![0x5a; 4096];
    let response = client
        .post(format!("{proxy_url}/services/{stream_service_name}/stream"))
        .bearer_auth(&stream_material.raw_key)
        .header("content-type", "application/octet-stream")
        .body(opaque_body.clone())
        .send()
        .await
        .expect("streaming-safe service response");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.bytes().await.expect("streamed response body"),
        opaque_body
    );

    let response = send_json(
        &client,
        &proxy_url,
        "/v1/chat/completions",
        Some(&material.raw_key),
        json!({
            "model": "coverage-model",
            "messages": [{"role": "user", "content": "x".repeat(2048)}]
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let error: Value = response.json().await.expect("overload response body");
    assert_eq!(error["error"]["code"], "gateway_overloaded");
    assert_eq!(error["error"]["retry_after_seconds"], 1);

    let response = send_json(
        &client,
        &proxy_url,
        &format!("/services/{service_name}/large-response"),
        Some(&material.raw_key),
        json!({"payload": "small"}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let error: Value = response.json().await.expect("response overload body");
    assert_eq!(error["error"]["code"], "gateway_overloaded");
    assert_eq!(error["error"]["retry_after_seconds"], 1);

    control_task.abort();
    upstream_task.abort();
}
