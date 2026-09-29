use axum::{extract::OriginalUri, http::HeaderMap, routing::any, Json, Router};
use gateway_core::{AdminOpenAiRouteStore, OpenAiRouteMode, SharedGatewayAuthRuntime};
use gateway_proxy::{PingoraLiteLlmConfig, RelaynaPingoraProxy};
use gateway_store::{PostgresStore, RedisControlState};
use pingora_core::server::Server;
use serde_json::{json, Value};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

async fn patch(store: &PostgresStore, value: Value) {
    store
        .patch_litellm_passthrough_settings(serde_json::from_value(value).unwrap())
        .await
        .unwrap();
}

// Raw requests preserve paths that URL clients would normalize before sending.
async fn request(port: u16, method: &str, path: &str, headers: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    stream
        .write_all(
            format!(
                "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n{headers}\r\n"
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let mut response = Vec::new();
    tokio::time::timeout(Duration::from_secs(10), stream.read_to_end(&mut response))
        .await
        .unwrap()
        .unwrap();
    let response = String::from_utf8(response).unwrap();
    (
        response.split_whitespace().nth(1).unwrap().parse().unwrap(),
        response,
    )
}

#[tokio::test]
async fn caller_key_policy_is_independent_persisted_and_never_escalates_credentials() {
    let (Ok(database_url), Ok(redis_url)) =
        (std::env::var("DATABASE_URL"), std::env::var("REDIS_URL"))
    else {
        eprintln!("skipping caller policy e2e: DATABASE_URL and REDIS_URL required");
        return;
    };
    let store = PostgresStore::connect(&database_url).await.unwrap();
    let mut lock = store.pool().acquire().await.unwrap();
    sqlx::query("SELECT pg_advisory_lock(82120260808)")
        .execute(&mut *lock)
        .await
        .unwrap();
    let previous = store.get_litellm_passthrough_settings().await.unwrap();
    let previous_routes = store.list_openai_route_settings().await.unwrap();
    let previous_identity = store.list_route_identities().await.unwrap();
    let captured = Arc::new(Mutex::new(Vec::<(String, HeaderMap)>::new()));
    let capture = captured.clone();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_url = format!("http://{}", listener.local_addr().unwrap());
    let mock = Router::new().fallback(any(
        move |OriginalUri(uri): OriginalUri, headers: HeaderMap| {
            let capture = capture.clone();
            async move {
                let credential = headers
                    .get("authorization")
                    .or_else(|| headers.get("x-upstream-key"))
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");
                let status = match credential {
                    "Bearer sk-caller" | "sk-caller" => 200,
                    "Bearer sk-limited" => 403,
                    "Bearer gateway-service-secret" => 200,
                    _ => 401,
                };
                capture.lock().unwrap().push((uri.to_string(), headers));
                (
                    axum::http::StatusCode::from_u16(status).unwrap(),
                    Json(json!({"upstream_status":status})),
                )
            }
        },
    ));
    let upstream_task = tokio::spawn(async move {
        axum::serve(listener, mock).await.unwrap();
    });
    // Keep this test independent of provider configuration left by other suites.
    let previous_providers: Vec<(uuid::Uuid, bool)> =
        sqlx::query_as("SELECT id, enabled FROM provider_configs WHERE provider='litellm'")
            .fetch_all(store.pool())
            .await
            .unwrap();
    sqlx::query("UPDATE provider_configs SET enabled=false WHERE provider='litellm'")
        .execute(store.pool())
        .await
        .unwrap();
    let provider_id = uuid::Uuid::new_v4();
    sqlx::query("INSERT INTO provider_configs (id,name,provider,base_url,credential_secret,enabled) VALUES ($1,$2,'litellm',$3,'gateway-service-secret',true)")
        .bind(provider_id).bind(format!("caller-policy-{provider_id}")).bind(&upstream_url).execute(store.pool()).await.unwrap();
    sqlx::query(
        "DELETE FROM route_identity_settings WHERE route IN ('/litellm/*','/v1/chat/completions')",
    )
    .execute(store.pool())
    .await
    .unwrap();
    patch(&store, json!({"enabled":true,"authentication_mode":"litellm_bearer","allowed_paths":["/*"],"allowed_methods":["GET","POST"],"ui_exposure":"disabled","admin_api_exposure":"explicitly_exposed","blocked_paths":["/key/delete","/config","/config/*","/v1/chat/completions"]})).await;
    let another_pod = PostgresStore::connect(&database_url).await.unwrap();
    assert_eq!(
        another_pod
            .get_litellm_passthrough_settings()
            .await
            .unwrap()
            .authentication_mode,
        gateway_core::LiteLlmAuthenticationMode::LitellmBearer
    );
    let port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    let auth = SharedGatewayAuthRuntime::new(Default::default()).unwrap();
    let config = PingoraLiteLlmConfig::from_base_url(&upstream_url, "fallback-service-secret")
        .unwrap()
        .with_auth_runtime(auth.clone());
    let proxy = RelaynaPingoraProxy::new(
        Arc::new(store.clone()),
        Arc::new(RedisControlState::new(&redis_url).unwrap()),
        config,
    );
    std::thread::spawn(move || {
        let mut server = Server::new(None).unwrap();
        server.bootstrap();
        let mut service = pingora_proxy::http_proxy_service(&server.configuration, proxy);
        service.add_tcp(&format!("127.0.0.1:{port}"));
        server.add_service(service);
        server.run_forever();
    });
    // Cold native test binaries can take longer to start a Pingora listener.
    for _ in 0..600 {
        if TcpStream::connect(("127.0.0.1", port)).await.is_ok() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let bearer = "Authorization: Bearer sk-caller\r\n";
    for exposure in [
        "disabled",
        "trusted_ingress",
        "explicitly_exposed",
        "operator_only",
    ] {
        patch(&another_pod, json!({"ui_exposure":exposure})).await;
        for path in [
            "/global/spend/logs",
            "/key/list?limit=10",
            "/login",
            "/models",
            "/uinfo",
        ] {
            let (status, response) = request(port, "GET", path, bearer).await;
            assert_eq!(status, 200, "{path}: {response}");
            let headers = captured.lock().unwrap().last().unwrap().1.clone();
            assert_eq!(headers.get("authorization").unwrap(), "Bearer sk-caller");
            assert!(!headers.contains_key("x-litellm-key"));
        }
    }
    patch(&store, json!({"ui_exposure":"disabled"})).await;
    let count = captured.lock().unwrap().len();
    for (method, path) in [
        ("GET", "/ui"),
        ("GET", "/ui/"),
        ("GET", "/ui/nested"),
        ("POST", "/key/delete"),
        ("GET", "/key/delete/"),
        ("GET", "/config?x=1"),
        ("GET", "/config/nested"),
        ("DELETE", "/key/list"),
        ("GET", "/%75i"),
        ("GET", "/ui%2fpage"),
        ("GET", "//ui"),
        ("GET", "/a/../ui"),
        ("GET", "/./ui"),
        ("GET", "/ui;param"),
        ("GET", "/ui\\page"),
        ("GET", "/%2575i"),
    ] {
        let (status, response) = request(port, method, path, bearer).await;
        assert_eq!(status, 403, "{path}: {response}");
        assert!(response.contains("policy_denied"));
    }
    for headers in [
        "",
        "Authorization: Basic secret\r\n",
        "Authorization: Bearer\r\n",
        "Authorization: Bearer bad token\r\n",
        "Authorization: Bearer sk-caller\r\nAuthorization: Bearer second\r\n",
    ] {
        let (status, body) = request(port, "GET", "/key/list", headers).await;
        assert_eq!(status, 401, "{body}");
        assert!(!body.contains("gateway-service-secret"));
    }
    assert_eq!(
        captured.lock().unwrap().len(),
        count,
        "local rejects must not reach upstream"
    );
    for (key, expected) in [("sk-invalid", 401), ("sk-limited", 403)] {
        let before = captured.lock().unwrap().len();
        let (status, body) = request(
            port,
            "GET",
            "/key/list",
            &format!("Authorization: Bearer {key}\r\n"),
        )
        .await;
        assert_eq!(status, expected);
        assert!(body.contains(&format!("\"upstream_status\":{expected}")));
        assert_eq!(
            captured.lock().unwrap().len(),
            before + 1,
            "no privileged retry"
        );
    }
    for exposure in ["disabled", "operator_only"] {
        patch(&store, json!({"admin_api_exposure":exposure})).await;
        assert_eq!(request(port, "GET", "/key/list", bearer).await.0, 403);
    }
    patch(&store, json!({"admin_api_exposure":"explicitly_exposed"})).await;
    // Canonical direct requests share the deny list even with wildcard forwarding disabled.
    store
        .set_openai_route_mode(
            "chat-completions",
            OpenAiRouteMode::DirectLiteLlmPassthrough,
        )
        .await
        .unwrap();
    patch(&store, json!({"enabled":false})).await;
    assert_eq!(
        request(port, "POST", "/v1/chat/completions", bearer)
            .await
            .0,
        403
    );
    patch(&store, json!({"blocked_paths":[]})).await;
    assert_eq!(
        request(port, "POST", "/v1/chat/completions", bearer)
            .await
            .0,
        200
    );
    patch(&store,json!({"enabled":true,"authentication_mode":"gateway","ui_exposure":"trusted_ingress","blocked_paths":["/key/list","/ui","/ui/*"]})).await;
    let count = captured.lock().unwrap().len();
    for path in ["/key/list", "/ui", "/ui/page"] {
        assert_eq!(request(port, "GET", path, bearer).await.0, 403);
    }
    assert_eq!(captured.lock().unwrap().len(), count);
    patch(&store, json!({"blocked_paths":[]})).await;
    assert_eq!(
        request(port, "GET", "/ui/", "").await.0,
        200,
        "legacy trusted ingress remains available"
    );
    assert_eq!(
        captured
            .lock()
            .unwrap()
            .last()
            .unwrap()
            .1
            .get("authorization")
            .unwrap(),
        "Bearer gateway-service-secret"
    );
    patch(
        &store,
        json!({"authentication_mode":"litellm_bearer","ui_exposure":"disabled"}),
    )
    .await;
    // Translation forwards the caller key and removes Authorization, never both.
    for (format, expected) in [("raw", "sk-caller"), ("bearer", "Bearer sk-caller")] {
        sqlx::query("UPDATE provider_configs SET credential_header_mode='custom_header',credential_header_name='x-upstream-key',credential_header_value_format=$2 WHERE id=$1").bind(provider_id).bind(format).execute(store.pool()).await.unwrap();
        assert_eq!(
            request(
                port,
                "GET",
                "/key/list",
                &format!("{bearer}x-upstream-key: spoofed\r\n")
            )
            .await
            .0,
            200
        );
        let headers = captured.lock().unwrap().last().unwrap().1.clone();
        assert!(!headers.contains_key("authorization"));
        assert_eq!(headers.get_all("x-upstream-key").iter().count(), 1);
        assert_eq!(headers.get("x-upstream-key").unwrap(), expected);
    }
    // Mandatory identity profiles are never bypassed by selecting caller mode.
    for access in [
        json!({"entra":{"audience":"api://test"}}),
        json!({"authentication_profiles":{"revision":0,"profiles":[{"id":"key","name":"Key","enabled":true,"type":"relayna_key_only"}],"bindings":[]}}),
    ] {
        store
            .set_route_identity(
                serde_json::from_value(json!({"route":"/litellm/*","access":access})).unwrap(),
            )
            .await
            .unwrap();
        let (status, body) = request(port, "GET", "/key/list", bearer).await;
        assert_eq!(status, 503, "{body}");
        assert!(body.contains("litellm_authentication_conflict"));
        sqlx::query("DELETE FROM route_identity_settings WHERE route='/litellm/*'")
            .execute(store.pool())
            .await
            .unwrap();
    }
    let mut runtime = auth.snapshot().unwrap().config;
    runtime.unverified_bearer_enabled = true;
    auth.update(runtime).unwrap();
    assert_eq!(request(port, "GET", "/key/list", bearer).await.0, 503);
    store
        .set_route_identity(
            serde_json::from_value(json!({"route":"/litellm/*","access":{"skip_entra":true}}))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(request(port, "GET", "/key/list", bearer).await.0, 200);
    auth.update(Default::default()).unwrap();
    // Keyless requests have inspectable Traffic but no invented key or billable usage row.
    let request_id = format!("caller-policy-{}", uuid::Uuid::new_v4());
    assert_eq!(
        request(
            port,
            "GET",
            "/key/list",
            &format!("{bearer}x-request-id: {request_id}\r\n")
        )
        .await
        .0,
        200
    );
    let mut traffic = None;
    for _ in 0..50 {
        traffic = sqlx::query_as::<_, (Option<uuid::Uuid>, Value)>(
            "SELECT key_id, record FROM request_traffic WHERE request_id=$1",
        )
        .bind(&request_id)
        .fetch_optional(store.pool())
        .await
        .unwrap();
        if traffic.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let (key_id, record) = traffic.expect("keyless traffic recorded");
    assert!(key_id.is_none());
    for secret in [
        "sk-caller",
        "gateway-service-secret",
        "fallback-service-secret",
    ] {
        assert!(
            !record.to_string().contains(secret),
            "traffic must redact credentials"
        );
    }
    let usage_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM usage_events WHERE request_id=$1")
            .bind(&request_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(usage_count, 0);
    // Clean up shared durable fixtures for other integration suites.
    patch(&store, serde_json::to_value(previous).unwrap()).await;
    for setting in previous_routes {
        store
            .set_openai_route_mode(&setting.route_id, setting.mode)
            .await
            .unwrap();
    }
    sqlx::query(
        "DELETE FROM route_identity_settings WHERE route IN ('/litellm/*','/v1/chat/completions')",
    )
    .execute(store.pool())
    .await
    .unwrap();
    for setting in previous_identity
        .into_iter()
        .filter(|s| matches!(s.route.as_str(), "/litellm/*" | "/v1/chat/completions"))
    {
        store.set_route_identity(setting).await.unwrap();
    }
    sqlx::query("DELETE FROM provider_configs WHERE id=$1")
        .bind(provider_id)
        .execute(store.pool())
        .await
        .unwrap();
    for (id, enabled) in previous_providers {
        sqlx::query("UPDATE provider_configs SET enabled=$2 WHERE id=$1")
            .bind(id)
            .bind(enabled)
            .execute(store.pool())
            .await
            .unwrap();
    }
    upstream_task.abort();
}
