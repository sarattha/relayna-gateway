use axum::{
    body::Bytes,
    extract::OriginalUri,
    http::{HeaderMap, StatusCode},
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
        .fallback(any(
            |OriginalUri(uri): OriginalUri, headers: HeaderMap| async move {
                (
                    StatusCode::OK,
                    Json(json!({
                        "id": "mock-response",
                        "auth": headers.get("authorization").and_then(|v| v.to_str().ok()),
                        "client_key": headers.contains_key("x-litellm-key"),
                        "path": uri.path(),
                        "model": "coverage-model",
                        "choices": [],
                        "usage": {"prompt_tokens": 2, "completion_tokens": 3, "total_tokens": 5},
                        "usage_metadata": {"total_cost": 0.001}
                    })),
                )
            },
        ));
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
    gateway_telemetry::init("gateway_proxy=warn", false);
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
    let auth_runtime = gateway_core::SharedGatewayAuthRuntime::new(Default::default()).unwrap();
    let proxy_config = PingoraLiteLlmConfig::from_base_url(&upstream_url, "litellm-secret")
        .expect("LiteLLM proxy config")
        .with_direct_openai(Some(
            PingoraUpstreamConfig::from_base_url(&upstream_url, "openai-secret")
                .expect("direct OpenAI config"),
        ))
        .with_auth_runtime(auth_runtime.clone())
        .with_worker_token(Some("worker-secret".to_owned()))
        .with_body_admission_limits(2, 512)
        .expect("body admission limits");
    let proxy = RelaynaPingoraProxy::new(
        Arc::new(store.clone()),
        Arc::new(redis_control),
        proxy_config,
    );
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
    // Control-plane readiness can precede the independently started Pingora listener.
    for attempt in 0..100 {
        if tokio::net::TcpStream::connect(("127.0.0.1", proxy_port))
            .await
            .is_ok()
        {
            break;
        }
        assert!(attempt < 99, "proxy listener did not become ready");
        time::sleep(Duration::from_millis(100)).await;
    }

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
    assert_eq!(
        response
            .headers()
            .get("connection")
            .and_then(|v| v.to_str().ok()),
        Some("close")
    );
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

    unverified_bearer_regressions(&client, &proxy_url, &material, &store, &auth_runtime).await;

    control_task.abort();
    upstream_task.abort();
}

// Runs against the same real Pingora process after the released native-mode cases.
async fn unverified_bearer_regressions(
    client: &reqwest::Client,
    proxy_url: &str,
    material: &VirtualKeyMaterial,
    store: &PostgresStore,
    runtime: &gateway_core::SharedGatewayAuthRuntime,
) {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use gateway_core::{
        AdminGatewayAuthSettingsStore, AdminProviderConfigStore, EffectiveGatewayAuthSettings,
        GatewayAuthEnv, LiteLlmCredentialMappingScope, LiteLlmCredentialMappingUpsertRequest,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};

    let rsa = openssl::rsa::Rsa::generate(2048).unwrap();
    let signing_key =
        jsonwebtoken::EncodingKey::from_rsa_pem(&rsa.private_key_to_pem().unwrap()).unwrap();
    let jwks = json!({"keys": [{"kty":"RSA", "kid":"issue114", "alg":"RS256", "use":"sig",
        "n": URL_SAFE_NO_PAD.encode(rsa.n().to_vec()), "e": URL_SAFE_NO_PAD.encode(rsa.e().to_vec())}]});
    let oidc_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let issuer = format!("http://{}", oidc_listener.local_addr().unwrap());
    let hits = Arc::new(AtomicUsize::new(0));
    let hits_for_server = hits.clone();
    let issuer_for_server = issuer.clone();
    let oidc = Router::new().fallback(any(move |OriginalUri(uri): OriginalUri| {
        hits_for_server.fetch_add(1, Ordering::SeqCst);
        let response = if uri.path() == "/keys" {
            jwks.clone()
        } else {
            json!({"issuer": issuer_for_server, "jwks_uri": format!("{issuer_for_server}/keys")})
        };
        async move { Json(response) }
    }));
    let oidc_task = tokio::spawn(async move {
        axum::serve(oidc_listener, oidc).await.unwrap();
    });
    let stored = store
        .patch_gateway_auth_settings(
            serde_json::from_value(json!({
                "unverified_bearer_enabled": true, "entra_enabled": true,
                "apigee_trusted_header_enabled": false, "relayna_key_header":"x-litellm-key",
                "tenant_id":"tenant", "audience":"api://gateway", "issuer":issuer,
                "oidc_discovery_url":format!("{issuer}/discovery"),
                "required_role":"gateway.invoke", "required_scope":"gateway.invoke",
                "allowed_groups":["operators"], "clock_skew_seconds":0
            }))
            .unwrap(),
        )
        .await
        .unwrap();
    // Read back from PostgreSQL rather than trusting the PATCH return value.
    assert!(
        store
            .gateway_auth_settings()
            .await
            .unwrap()
            .unwrap()
            .unverified_bearer_enabled
    );
    let effective =
        EffectiveGatewayAuthSettings::from_sources(Some(stored), &GatewayAuthEnv::default())
            .unwrap();
    runtime.update(effective.runtime_config()).unwrap();
    assert!(runtime.snapshot().unwrap().entra_verifier.is_none());
    let now = chrono::Utc::now().timestamp();
    let claims = json!({"iss":issuer,"aud":"api://gateway","tid":"tenant","oid":"user",
        "sub":"user","ver":"2.0","exp":now+3600,"nbf":now-10,"iat":now-10,
        "roles":["gateway.invoke"],"scp":"gateway.invoke","groups":["operators"]});
    let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256);
    header.kid = Some("issue114".to_owned());
    let sign = |claims: &Value| jsonwebtoken::encode(&header, claims, &signing_key).unwrap();
    let valid = sign(&claims);
    let mut tokens = vec!["not-a-jwt".to_owned()];
    for (field, value) in [
        ("iss", json!("https://wrong.example")),
        ("aud", json!("wrong")),
        ("exp", json!(now - 3600)),
        ("roles", json!([])),
        ("scp", json!("wrong")),
        ("groups", json!([])),
    ] {
        let mut invalid = claims.clone();
        invalid[field] = value;
        tokens.push(sign(&invalid));
    }
    let other = openssl::rsa::Rsa::generate(2048).unwrap();
    tokens.push(
        jsonwebtoken::encode(
            &header,
            &claims,
            &jsonwebtoken::EncodingKey::from_rsa_pem(&other.private_key_to_pem().unwrap()).unwrap(),
        )
        .unwrap(),
    );
    let request = |authorization: Option<&str>, key: Option<&str>| {
        let mut request = client
            .post(format!("{proxy_url}/v1/chat/completions"))
            .json(&json!({"model":"coverage-model","messages":[]}));
        if let Some(value) = authorization {
            request = request.header("authorization", value);
        }
        if let Some(value) = key {
            request = request.header("x-litellm-key", value);
        }
        request
    };
    for token in tokens.iter().chain(std::iter::once(&valid)) {
        let response = request(Some(&format!("Bearer {token}")), Some(&material.raw_key))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body: Value = response.json().await.unwrap();
        assert_eq!(body["auth"], "Bearer litellm-secret");
        assert_eq!(body["client_key"], false);
    }
    assert_eq!(
        hits.load(Ordering::SeqCst),
        0,
        "paused mode must not fetch discovery/JWKS"
    );
    for authorization in [None, Some(""), Some("Bearer "), Some("Basic token")] {
        assert_eq!(
            request(authorization, Some(&material.raw_key))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }
    for key in [
        None,
        Some(""),
        Some("invalid"),
        Some("sk-not-a-relayna-key"),
    ] {
        assert_eq!(
            request(Some("Bearer unverified"), key)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }
    let unknown = VirtualKeyMaterial::generate().unwrap();
    assert_eq!(
        request(Some("Bearer unverified"), Some(&unknown.raw_key))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    for assignment in [
        "disabled = true",
        "revoked_at = now()",
        "expires_at = now() - interval '1 hour'",
    ] {
        sqlx::query(&format!(
            "UPDATE api_keys SET {assignment} WHERE key_prefix = $1"
        ))
        .bind(&material.key_prefix)
        .execute(store.pool())
        .await
        .unwrap();
        assert_eq!(
            request(Some("Bearer unverified"), Some(&material.raw_key))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        sqlx::query("UPDATE api_keys SET disabled = false, revoked_at = NULL, expires_at = NULL WHERE key_prefix = $1")
            .bind(&material.key_prefix).execute(store.pool()).await.unwrap();
    }
    // A valid key is still subject to its provider policy.
    sqlx::query("UPDATE key_policies SET allowed_providers = ARRAY['openai-compatible'] WHERE key_id = (SELECT id FROM api_keys WHERE key_prefix = $1)")
        .bind(&material.key_prefix).execute(store.pool()).await.unwrap();
    assert_eq!(
        request(Some("Bearer unverified"), Some(&material.raw_key))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    sqlx::query("UPDATE key_policies SET allowed_providers = ARRAY['litellm', 'openai-compatible', 'internal-service'] WHERE key_id = (SELECT id FROM api_keys WHERE key_prefix = $1)")
        .bind(&material.key_prefix).execute(store.pool()).await.unwrap();
    let key_id: Uuid = sqlx::query_scalar("SELECT id FROM api_keys WHERE key_prefix = $1")
        .bind(&material.key_prefix)
        .fetch_one(store.pool())
        .await
        .unwrap();
    store
        .upsert_litellm_credential_mapping(LiteLlmCredentialMappingUpsertRequest {
            scope: LiteLlmCredentialMappingScope::Key,
            target_id: key_id,
            enabled: true,
            credential: Some("sk-issue114-mapped".to_owned()),
        })
        .await
        .unwrap();
    let response = request(Some("Bearer unverified"), Some(&material.raw_key))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["auth"], "Bearer sk-issue114-mapped");
    assert_eq!(body["client_key"], false);

    let stored = store
        .patch_gateway_auth_settings(
            serde_json::from_str(r#"{"unverified_bearer_enabled":false}"#).unwrap(),
        )
        .await
        .unwrap();
    let restored =
        EffectiveGatewayAuthSettings::from_sources(Some(stored), &GatewayAuthEnv::default())
            .unwrap();
    assert_eq!(restored.entra_auth, effective.entra_auth);
    runtime.update(restored.runtime_config()).unwrap();
    let response = request(Some(&format!("Bearer {valid}")), Some(&material.raw_key))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let _ = response.bytes().await.unwrap();
    for token in &tokens {
        let response = request(Some(&format!("Bearer {token}")), Some(&material.raw_key))
            .send()
            .await
            .unwrap();
        assert!(
            response.status() == StatusCode::UNAUTHORIZED
                || response.status() == StatusCode::FORBIDDEN,
            "restored verification accepted an invalid token: {}",
            response.status()
        );
    }
    assert!(hits.load(Ordering::SeqCst) >= 2);
    // Leave shared persisted settings in their default state for other integration cases.
    store
        .patch_gateway_auth_settings(serde_json::from_str(r#"{"entra_enabled":false}"#).unwrap())
        .await
        .unwrap();
    oidc_task.abort();
}
