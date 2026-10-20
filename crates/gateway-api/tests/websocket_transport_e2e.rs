//! Real Pingora transport with disposable PostgreSQL/Redis and a mock upstream.
use axum::{
    extract::{
        ws::{rejection::WebSocketUpgradeRejection, Message as UpstreamMessage, WebSocketUpgrade},
        OriginalUri, State,
    },
    http::HeaderMap,
    response::IntoResponse,
    routing::any,
    Json, Router,
};
use futures_util::{SinkExt, StreamExt};
use gateway_core::*;
use gateway_proxy::{PingoraLiteLlmConfig, RelaynaPingoraProxy};
use gateway_store::{PostgresStore, RedisControlState};
use serde_json::{json, Value};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, Error, Message},
};
use uuid::Uuid;

type Captured = Arc<Mutex<Vec<(String, HeaderMap)>>>;

async fn upstream(
    State(captured): State<Captured>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    upgrade: Result<WebSocketUpgrade, WebSocketUpgradeRejection>,
) -> axum::response::Response {
    let credential = headers
        .get("authorization")
        .or_else(|| headers.get("x-upstream-key"))
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let permitted = matches!(
        credential,
        "Bearer service-only" | "Bearer sk-caller" | "Bearer sk-other" | "sk-caller"
    );
    captured.lock().unwrap().push((uri.to_string(), headers));
    if !permitted {
        return http::StatusCode::UNAUTHORIZED.into_response();
    }
    if uri.path().ends_with("/reject") {
        return (
            http::StatusCode::FORBIDDEN,
            Json(json!({"error":"upstream denied"})),
        )
            .into_response();
    }
    match upgrade {
        Ok(upgrade) => upgrade
            .protocols(["echo.v1"])
            .on_upgrade(|mut socket| async move {
                while let Some(Ok(message)) = socket.recv().await {
                    let response = match message {
                        UpstreamMessage::Text(ref text) if text.as_str() == "__large__" => {
                            UpstreamMessage::Binary(vec![0; 256].into())
                        }
                        UpstreamMessage::Ping(bytes) => UpstreamMessage::Pong(bytes),
                        UpstreamMessage::Close(_) => break,
                        message => message,
                    };
                    if socket.send(response).await.is_err() {
                        break;
                    }
                }
            })
            .into_response(),
        Err(_) => Json(json!({"path":uri.path(),"query":uri.query()})).into_response(),
    }
}

fn request(base: &str, path: &str, credential: Option<&str>, id: &str) -> http::Request<()> {
    let mut req = format!("{}{path}", base.replacen("http:", "ws:", 1))
        .into_client_request()
        .unwrap();
    req.headers_mut()
        .insert("x-request-id", id.parse().unwrap());
    if let Some(credential) = credential {
        req.headers_mut().insert(
            "authorization",
            format!("Bearer {credential}").parse().unwrap(),
        );
    }
    req
}

async fn denied(req: http::Request<()>, expected: u16) {
    match tokio::time::timeout(Duration::from_secs(5), connect_async(req))
        .await
        .unwrap()
    {
        Err(Error::Http(response)) => {
            assert_eq!(response.status().as_u16(), expected, "{response:?}")
        }
        other => panic!("expected HTTP {expected}, got {other:?}"),
    }
}

async fn route_access(store: &PostgresStore, route: &str, access: Value) {
    store
        .set_route_identity(serde_json::from_value(json!({"route":route,"access":access})).unwrap())
        .await
        .unwrap();
}

async fn settings(store: &PostgresStore, value: Value) {
    store
        .patch_litellm_passthrough_settings(serde_json::from_value(value).unwrap())
        .await
        .unwrap();
}

async fn traffic(store: &PostgresStore, id: &str) -> Value {
    for _ in 0..100 {
        let record: Option<Value> =
            sqlx::query_scalar("SELECT record FROM request_traffic WHERE request_id=$1")
                .bind(id)
                .fetch_optional(store.pool())
                .await
                .unwrap();
        if let Some(record) = record {
            return record;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("missing terminal Traffic record {id}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn general_service_and_caller_litellm_websocket_transport() {
    let (Ok(database), Ok(redis)) = (std::env::var("DATABASE_URL"), std::env::var("REDIS_URL"))
    else {
        eprintln!("set disposable DATABASE_URL and REDIS_URL for WebSocket transport E2E");
        return;
    };
    let store = PostgresStore::connect(&database).await.unwrap();
    let mut lock = store.pool().acquire().await.unwrap();
    sqlx::query("SELECT pg_advisory_lock(82120260808)")
        .execute(&mut *lock)
        .await
        .unwrap();
    let previous_settings = store.get_litellm_passthrough_settings().await.unwrap();
    let previous_routes = store.list_openai_route_settings().await.unwrap();
    let previous_identities = store.list_route_identities().await.unwrap();
    let previous_providers: Vec<(Uuid, bool)> =
        sqlx::query_as("SELECT id,enabled FROM provider_configs WHERE provider='litellm'")
            .fetch_all(store.pool())
            .await
            .unwrap();
    sqlx::query("UPDATE provider_configs SET enabled=false WHERE provider='litellm'")
        .execute(store.pool())
        .await
        .unwrap();
    let captured: Captured = Default::default();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_url = format!("http://{}", listener.local_addr().unwrap());
    let app = Router::new()
        .fallback(any(upstream))
        .with_state(captured.clone());
    let upstream_task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let provider_id = Uuid::new_v4();
    sqlx::query("INSERT INTO provider_configs(id,name,provider,base_url,credential_secret,enabled) VALUES($1,$2,'litellm',$3,'stored-provider-only',true)")
        .bind(provider_id).bind(format!("ws-provider-{provider_id}")).bind(&upstream_url).execute(store.pool()).await.unwrap();
    let name = format!("ws-{}", Uuid::new_v4().simple());
    let project = store
        .create_project(ProjectCreateRequest { name: name.clone() })
        .await
        .unwrap();
    store
        .upsert_policy_layer(AdminPolicyLayerUpsert {
            kind: PolicyLayerKind::Project,
            scope_id: Some(project.id.to_string()),
            policy: Default::default(),
            guardrail_policy: Default::default(),
        })
        .await
        .unwrap();
    let socket_settings = json!({"idle_timeout_ms":1000,"max_connections":2,"max_connections_per_key":1,"max_frame_bytes":125,"max_session_duration_seconds":60});
    store.create_service(serde_json::from_value(json!({"name":name,"project_id":project.id,"upstream_base_url":upstream_url,"credential":"service-only","allowed_methods":["GET","POST"],"cost_mode":"fixed","estimated_cost_usd":0.01,"access":{"websocket":socket_settings}})).unwrap()).await.unwrap();
    let mut keys = Vec::new();
    for _ in 0..3 {
        let material = VirtualKeyMaterial::generate().unwrap();
        let key = store.create_admin_key(serde_json::from_value(json!({"owner_type":"project","project_id":project.id,"service_names":[name],"policy":{"allowed_routes":["/services/*"],"allowed_providers":["internal-service"],"allowed_services":[name],"allow_streaming":true,"monthly_budget_usd":5,"rpm_limit":100}})).unwrap(), &material).await.unwrap();
        keys.push((key.id, material.raw_key));
    }
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let proxy = RelaynaPingoraProxy::new(
        Arc::new(store.clone()),
        Arc::new(RedisControlState::new(&redis).unwrap()),
        PingoraLiteLlmConfig::from_base_url(&upstream_url, "stored-fallback-only").unwrap(),
    );
    std::thread::spawn(move || {
        let mut server = pingora_core::server::Server::new(None).unwrap();
        server.bootstrap();
        let mut service = pingora_proxy::http_proxy_service(&server.configuration, proxy);
        service.add_tcp(&format!("127.0.0.1:{port}"));
        server.add_service(service);
        server.run_forever();
    });
    for _ in 0..600 {
        if TcpStream::connect(("127.0.0.1", port)).await.is_ok() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let base = format!("http://127.0.0.1:{port}");
    let path = format!("/services/{name}/echo?channel=a");
    // Existing HTTP on the same route retains normal fixed pricing.
    let http = reqwest::Client::new()
        .get(format!("{base}{path}"))
        .bearer_auth(&keys[0].1)
        .send()
        .await
        .unwrap();
    assert_eq!(http.status(), 200);
    assert_eq!(
        http.json::<Value>().await.unwrap(),
        json!({"path":"/echo","query":"channel=a"})
    );
    denied(request(&base, &path, None, "ws-no-key"), 401).await;
    let id = format!("ws-service-{}", Uuid::new_v4());
    let mut req = request(&base, &path, Some(&keys[0].1), &id);
    for (name, value) in [
        ("sec-websocket-protocol", "echo.v1"),
        ("sec-websocket-extensions", "permessage-deflate"),
        ("x-api-key", "spoofed"),
        ("cookie", "session=spoofed"),
    ] {
        req.headers_mut().insert(name, value.parse().unwrap());
    }
    let (mut socket, response) = connect_async(req).await.unwrap();
    assert_eq!(response.status(), 101);
    assert_eq!(response.headers()["sec-websocket-protocol"], "echo.v1");
    assert!(!response.headers().contains_key("sec-websocket-extensions"));
    let (uri, headers) = captured.lock().unwrap().last().unwrap().clone();
    assert_eq!(uri, "/echo?channel=a");
    assert_eq!(headers["authorization"], "Bearer service-only");
    for name in [
        "x-api-key",
        "cookie",
        "x-relayna-key",
        "sec-websocket-extensions",
    ] {
        assert!(!headers.contains_key(name));
    }
    for message in [
        Message::Text("hello".into()),
        Message::Binary(vec![1, 2, 3].into()),
        Message::Ping(vec![4].into()),
    ] {
        socket.send(message.clone()).await.unwrap();
        let received = tokio::time::timeout(Duration::from_secs(3), socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let expected = match message {
            Message::Ping(bytes) => Message::Pong(bytes),
            message => message,
        };
        assert_eq!(received, expected);
    }
    denied(request(&base, &path, Some(&keys[0].1), "ws-key-cap"), 429).await;
    let (mut second, _) = connect_async(request(&base, &path, Some(&keys[1].1), "ws-second"))
        .await
        .unwrap();
    denied(request(&base, &path, Some(&keys[2].1), "ws-route-cap"), 429).await;
    second.close(None).await.unwrap();
    drop(second);
    socket.close(None).await.unwrap();
    drop(socket);
    let record = traffic(&store, &id).await;
    assert_eq!(record["diagnostics"]["protocol"], "websocket");
    assert_eq!(record["diagnostics"]["websocket"]["state"], "closed");
    assert!(
        record["diagnostics"]["websocket"]["client_frames"]
            .as_u64()
            .unwrap()
            >= 3
    );
    let usage: (Option<i64>, Option<f64>) = sqlx::query_as(
        "SELECT input_tokens,estimated_cost::float8 FROM usage_events WHERE request_id=$1",
    )
    .bind(&id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(usage, (None, None));
    assert!(!record.to_string().contains("service-only"));
    // Rejected handshakes release their connection slot; upstream status passes through.
    denied(
        request(
            &base,
            &format!("/services/{name}/reject"),
            Some(&keys[0].1),
            "ws-upstream-denied",
        ),
        403,
    )
    .await;
    for (message, failure) in [
        (
            Message::Binary(vec![0; 126].into()),
            "websocket_frame_rejected",
        ),
        (
            Message::Text("__large__".into()),
            "websocket_frame_rejected",
        ),
    ] {
        let id = Uuid::new_v4().to_string();
        let (mut socket, _) = connect_async(request(&base, &path, Some(&keys[0].1), &id))
            .await
            .unwrap();
        let _ = socket.send(message).await;
        let result = tokio::time::timeout(Duration::from_secs(3), socket.next())
            .await
            .unwrap();
        assert!(!matches!(result, Some(Ok(Message::Binary(_)))));
        drop(socket);
        assert_eq!(
            traffic(&store, &id).await["diagnostics"]["failure_code"],
            failure
        );
    }
    let id = Uuid::new_v4().to_string();
    let (mut idle, _) = connect_async(request(&base, &path, Some(&keys[0].1), &id))
        .await
        .unwrap();
    let result = tokio::time::timeout(Duration::from_secs(3), idle.next()).await;
    assert!(result.is_ok(), "idle connection must close");
    drop(idle);
    traffic(&store, &id).await;
    // Active sessions enforce age without interpreting application messages.
    let mut short_session = socket_settings.clone();
    // Leave room for admission work and whole-second deadline rounding under
    // parallel test load; the idle timeout must exceed the session lifetime.
    short_session["max_session_duration_seconds"] = json!(5);
    short_session["idle_timeout_ms"] = json!(10000);
    store
        .patch_service(
            &name,
            serde_json::from_value(json!({"access":{"websocket":short_session}})).unwrap(),
        )
        .await
        .unwrap();
    let id = Uuid::new_v4().to_string();
    let (mut expired, _) = connect_async(request(&base, &path, Some(&keys[0].1), &id))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(5100)).await;
    let _ = expired.send(Message::Text("after expiry".into())).await;
    let _ = tokio::time::timeout(Duration::from_secs(3), expired.next())
        .await
        .unwrap();
    drop(expired);
    assert_eq!(
        traffic(&store, &id).await["diagnostics"]["failure_code"],
        "websocket_session_expired"
    );
    store
        .patch_service(
            &name,
            serde_json::from_value(json!({"access":{"websocket":socket_settings}})).unwrap(),
        )
        .await
        .unwrap();
    // HTTP content guardrails cannot be bypassed by switching transport.
    store
        .patch_admin_key(
            keys[0].0,
            serde_json::from_value(
                json!({"guardrail_policy":{"mandatory_guardrails":["pii-redact"]}}),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    let count = captured.lock().unwrap().len();
    denied(
        request(&base, &path, Some(&keys[0].1), "ws-guardrail-denied"),
        GatewayError::GuardrailUnavailable.status_code().as_u16(),
    )
    .await;
    assert_eq!(captured.lock().unwrap().len(), count);
    store
        .patch_admin_key(
            keys[0].0,
            serde_json::from_value(json!({"guardrail_policy":{"mandatory_guardrails":[]}}))
                .unwrap(),
        )
        .await
        .unwrap();
    // Streaming policy and HTTP-only defaults are enforced before contacting upstream.
    store
        .patch_admin_key(
            keys[0].0,
            serde_json::from_value(json!({"policy":{"allow_streaming":false}})).unwrap(),
        )
        .await
        .unwrap();
    let count = captured.lock().unwrap().len();
    denied(
        request(&base, &path, Some(&keys[0].1), "ws-stream-denied"),
        403,
    )
    .await;
    assert_eq!(captured.lock().unwrap().len(), count);
    store
        .patch_admin_key(
            keys[0].0,
            serde_json::from_value(json!({"policy":{"allow_streaming":true}})).unwrap(),
        )
        .await
        .unwrap();
    store
        .patch_service(&name, serde_json::from_value(json!({"access":{}})).unwrap())
        .await
        .unwrap();
    denied(request(&base, &path, Some(&keys[0].1), "ws-http-only"), 404).await;
    // Caller-owned LiteLLM keys delegate sessions without a Relayna billing identity.
    settings(&store,json!({"enabled":true,"authentication_mode":"litellm_bearer","allowed_paths":["/v1/*","/responses"],"allowed_methods":["GET","POST"],"blocked_paths":[],"ui_exposure":"disabled","admin_api_exposure":"disabled"})).await;
    route_access(&store, "/litellm/*", json!({"websocket":socket_settings})).await;
    route_access(
        &store,
        "/v1/responses",
        json!({"websocket":socket_settings}),
    )
    .await;
    let litepath = "/v1/realtime?model=mock";
    let id = Uuid::new_v4().to_string();
    let (mut caller, _) = connect_async(request(&base, litepath, Some("sk-caller"), &id))
        .await
        .unwrap();
    caller
        .send(Message::Text("response.create".into()))
        .await
        .unwrap();
    assert_eq!(
        caller.next().await.unwrap().unwrap(),
        Message::Text("response.create".into())
    );
    assert_eq!(
        captured.lock().unwrap().last().unwrap().1["authorization"],
        "Bearer sk-caller"
    );
    denied(
        request(&base, litepath, Some("sk-caller"), "ws-lite-cap"),
        429,
    )
    .await;
    caller.close(None).await.unwrap();
    drop(caller);
    let record = traffic(&store, &id).await;
    assert!(record["key_id"].is_null());
    assert_eq!(record["diagnostics"]["websocket"]["state"], "closed");
    assert!(!record.to_string().contains("sk-caller"));
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM usage_events WHERE request_id=$1")
        .bind(&id)
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(count, 0);
    denied(
        request(&base, litepath, Some("sk-invalid"), "ws-lite-invalid"),
        401,
    )
    .await;
    denied(
        request(&base, litepath, Some(&keys[0].1), "ws-lite-relayna-key"),
        401,
    )
    .await;
    settings(&store, json!({"blocked_paths":["/v1/realtime"]})).await;
    denied(
        request(&base, litepath, Some("sk-caller"), "ws-lite-blocked"),
        403,
    )
    .await;
    settings(
        &store,
        json!({"blocked_paths":[],"allowed_methods":["POST"]}),
    )
    .await;
    denied(
        request(&base, litepath, Some("sk-caller"), "ws-lite-method"),
        403,
    )
    .await;
    settings(
        &store,
        json!({"allowed_methods":["GET","POST"],"authentication_mode":"gateway"}),
    )
    .await;
    denied(
        request(&base, litepath, Some(&keys[0].1), "ws-lite-governed"),
        404,
    )
    .await;
    settings(&store, json!({"authentication_mode":"litellm_bearer"})).await;
    route_access(
        &store,
        "/litellm/*",
        json!({"websocket":socket_settings,"entra":{"audience":"api://required"}}),
    )
    .await;
    denied(
        request(
            &base,
            litepath,
            Some("sk-caller"),
            "ws-lite-required-identity",
        ),
        503,
    )
    .await;
    route_access(&store, "/litellm/*", json!({"websocket":socket_settings})).await;
    // Canonical mode/enablement cannot be bypassed by wildcard socket forwarding.
    store
        .set_openai_route_mode("responses", OpenAiRouteMode::ManagedByGateway)
        .await
        .unwrap();
    denied(
        request(
            &base,
            "/v1/responses?model=mock",
            Some("sk-caller"),
            "ws-managed-responses",
        ),
        404,
    )
    .await;
    store
        .set_openai_route_mode("responses", OpenAiRouteMode::DirectLiteLlmPassthrough)
        .await
        .unwrap();
    store
        .set_openai_route_enabled("responses", false)
        .await
        .unwrap();
    denied(
        request(
            &base,
            "/v1/responses?model=mock",
            Some("sk-caller"),
            "ws-disabled-responses",
        ),
        403,
    )
    .await;
    store
        .set_openai_route_enabled("responses", true)
        .await
        .unwrap();
    for path in ["/v1/responses?model=mock", "/responses?model=mock"] {
        let id = Uuid::new_v4().to_string();
        let (mut socket, _) = connect_async(request(&base, path, Some("sk-caller"), &id))
            .await
            .unwrap();
        socket.close(None).await.unwrap();
        drop(socket);
        traffic(&store, &id).await;
    }
    // Header translation preserves caller credentials and removes spoofed header values.
    sqlx::query("UPDATE provider_configs SET credential_header_mode='custom_header',credential_header_name='x-upstream-key' WHERE id=$1").bind(provider_id).execute(store.pool()).await.unwrap();
    let mut req = request(&base, litepath, Some("sk-caller"), "ws-custom-header");
    req.headers_mut()
        .insert("x-upstream-key", "spoofed".parse().unwrap());
    let (mut socket, _) = connect_async(req).await.unwrap();
    let headers = captured.lock().unwrap().last().unwrap().1.clone();
    assert_eq!(headers["x-upstream-key"], "sk-caller");
    assert!(!headers.contains_key("authorization"));
    socket.close(None).await.unwrap();
    drop(socket);
    traffic(&store, "ws-custom-header").await;
    // Restore shared settings for the existing integration suites.
    settings(&store, serde_json::to_value(previous_settings).unwrap()).await;
    for setting in previous_routes {
        store
            .set_openai_route_mode(&setting.route_id, setting.mode)
            .await
            .unwrap();
        store
            .set_openai_route_enabled(&setting.route_id, setting.enabled)
            .await
            .unwrap();
    }
    sqlx::query(
        "DELETE FROM route_identity_settings WHERE route IN ('/litellm/*','/v1/responses')",
    )
    .execute(store.pool())
    .await
    .unwrap();
    for setting in previous_identities
        .into_iter()
        .filter(|setting| matches!(setting.route.as_str(), "/litellm/*" | "/v1/responses"))
    {
        store.set_route_identity(setting).await.unwrap();
    }
    for (id, _) in keys {
        store.revoke_admin_key(id).await.unwrap();
    }
    store.delete_service(&name).await.unwrap();
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
