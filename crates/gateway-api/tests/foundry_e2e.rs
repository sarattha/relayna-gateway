//! Foundry contract tests through real admin HTTP, Pingora, PostgreSQL and Redis.
use axum::{
    body::Body,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use gateway_core::admin::KeyPolicyPatch;
use gateway_core::traffic::TrafficStore;
use gateway_core::*;
use gateway_proxy::{PingoraLiteLlmConfig, RelaynaPingoraProxy};
use gateway_store::{PostgresStore, RedisControlState, RedisReadiness};
use serde_json::{json, Value};
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use uuid::Uuid;

#[derive(Default)]
struct Mock {
    tokens: AtomicUsize,
    reads: AtomicUsize,
    read_status: AtomicUsize,
    calls: Mutex<Vec<(HeaderMap, Value)>>,
}
async fn serve(app: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}
async fn token(
    State(state): State<Arc<Mock>>,
    axum::extract::Form(form): axum::extract::Form<std::collections::HashMap<String, String>>,
) -> axum::response::Response {
    assert_eq!(form["grant_type"], "client_credentials");
    assert_eq!(form["scope"], "https://ai.azure.com/.default");
    state.tokens.fetch_add(1, Ordering::SeqCst);
    if form["client_secret"] == "invalid-secret" {
        return (StatusCode::UNAUTHORIZED, "never leak token error details").into_response();
    }
    assert_eq!(form["client_secret"], "mock-client-secret");
    Json(json!({"access_token":"mock-azure-access-token","token_type":"Bearer","expires_in":3}))
        .into_response()
}
async fn agents(
    State(state): State<Arc<Mock>>,
    headers: HeaderMap,
    axum::extract::Query(query): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> axum::response::Response {
    state.reads.fetch_add(1, Ordering::SeqCst);
    assert_eq!(headers["authorization"], "Bearer mock-azure-access-token");
    assert_eq!(query["api-version"], "v1");
    assert_eq!(query["limit"], "1");
    assert!(!headers.contains_key("cookie"));
    let status = state.read_status.load(Ordering::SeqCst);
    if status == 0 {
        return Json(json!({"data":[], "nextLink":"https://never-follow.example"})).into_response();
    }
    (
        StatusCode::from_u16(status as u16).unwrap(),
        "private upstream details",
    )
        .into_response()
}
async fn responses(
    State(state): State<Arc<Mock>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> axum::response::Response {
    state
        .calls
        .lock()
        .unwrap()
        .push((headers.clone(), body.clone()));
    assert_eq!(headers["authorization"], "Bearer mock-azure-access-token");
    for name in [
        "x-relayna-key",
        "api-key",
        "x-api-key",
        "cookie",
        "x-apigee-entra-identity",
    ] {
        assert!(!headers.contains_key(name), "{name} leaked");
    }
    assert_eq!(body["store"], false);
    if body["input"] == "fail" {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            [("retry-after", "2")],
            Json(json!({"error":{"code":"rate_limit","message":"mock capacity"}})),
        )
            .into_response();
    }
    if body["stream"] == true {
        let stream = futures_util::stream::unfold(0, |step| async move {
            match step {
                0 => Some((Ok::<_, std::io::Error>("event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"Grounded answer\"}\n\n"), 1)),
                1 => { tokio::time::sleep(Duration::from_millis(700)).await; Some((Ok("event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"output\":[{\"content\":[{\"annotations\":[{\"type\":\"url_citation\",\"url\":\"https://example.com/source\"}]}]}],\"usage\":{\"input_tokens\":2,\"output_tokens\":3,\"total_tokens\":5}}}\n\n"), 2)) },
                _ => None,
            }
        });
        return (
            [("content-type", "text/event-stream")],
            Body::from_stream(stream),
        )
            .into_response();
    }
    Json(json!({"id":"resp_mock", "object":"response", "output":[{"type":"message","content":[{"type":"output_text","text":"Grounded answer","annotations":[{"type":"url_citation","url":"https://example.com/source","title":"Source"}]}]}], "usage":{"input_tokens":2,"output_tokens":3,"total_tokens":5}})).into_response()
}
async fn admin(
    client: &reqwest::Client,
    base: &str,
    token: &str,
    method: reqwest::Method,
    path: &str,
    body: Value,
    status: u16,
) -> Value {
    let response = client
        .request(method, format!("{base}/admin-ui/admin/{path}"))
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .unwrap();
    eprintln!("admin {path} {}", response.status());
    let actual = response.status().as_u16();
    let text = response.text().await.unwrap();
    assert_eq!(actual, status, "{path}: {text}");
    if text.is_empty() {
        Value::Null
    } else {
        serde_json::from_str(&text).unwrap()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn foundry_registered_agents_and_passthrough_are_governed_and_stream_without_buffering() {
    let (Ok(database), Ok(redis)) = (std::env::var("DATABASE_URL"), std::env::var("REDIS_URL"))
    else {
        return;
    };
    let parent = sqlx::PgPool::connect(&database).await.unwrap();
    let name = format!("foundry_{}", Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE DATABASE {name}"))
        .execute(&parent)
        .await
        .unwrap();
    let mut url = url::Url::parse(&database).unwrap();
    url.set_path(&name);
    let store = PostgresStore::connect(url.as_str()).await.unwrap();
    let mock = Arc::new(Mock::default());
    let upstream = serve(
        Router::new()
            .route("/token", post(token))
            .route("/api/projects/demo/agents", get(agents))
            .route("/api/projects/demo/openai/v1/responses", post(responses))
            .with_state(mock.clone()),
    )
    .await;
    std::env::set_var(
        "GATEWAY_FOUNDRY_MOCK_TOKEN_ENDPOINT",
        format!("{upstream}/token"),
    );
    let operator = OperatorTokenMaterial::generate().unwrap();
    store.bootstrap_operator_token(&operator).await.unwrap();
    store
        .upsert_policy_layer(AdminPolicyLayerUpsert {
            kind: PolicyLayerKind::Global,
            scope_id: None,
            policy: Default::default(),
            guardrail_policy: Default::default(),
        })
        .await
        .unwrap();
    let auth = SharedGatewayAuthRuntime::new(GatewayAuthRuntimeConfig {
        unverified_bearer_enabled: false,
        relayna_key_header: "x-relayna-key".into(),
        entra_auth: None,
        apigee_trusted_header: None,
    })
    .unwrap();
    let admin_url = serve(gateway_api::app::router_with_studio_auth_and_litellm(
        store.clone(),
        RedisReadiness::new(&redis).unwrap(),
        None,
        GatewayAuthEnv::default(),
        auth.clone(),
        upstream.clone(),
        "unused".into(),
    ))
    .await;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        .build()
        .unwrap();
    let provider = admin(&client,&admin_url,&operator.raw_token,reqwest::Method::POST,"providers",json!({"provider":"azure-foundry","name":"Mock Foundry","base_url":format!("{upstream}/api/projects/demo"),"credential":"mock-client-secret","foundry":{"method":"client_secret","tenant_id":Uuid::nil(),"client_id":Uuid::nil()}}),200).await;
    assert!(!provider.to_string().contains("mock-client-secret"));
    let provider_id = provider["id"].as_str().unwrap();
    // API clients omitting credential must also clear it when leaving secret auth.
    for method in ["workload_identity", "managed_identity"] {
        let changed = admin(
            &client,
            &admin_url,
            &operator.raw_token,
            reqwest::Method::PATCH,
            &format!("providers/{provider_id}"),
            json!({"foundry":{"method":method,"tenant_id":Uuid::nil(),"client_id":Uuid::nil()}}),
            200,
        )
        .await;
        assert_eq!(changed["credential_configured"], false);
        let stored: Option<String> =
            sqlx::query_scalar("SELECT credential_secret FROM provider_configs WHERE id=$1")
                .bind(Uuid::parse_str(provider_id).unwrap())
                .fetch_one(store.pool())
                .await
                .unwrap();
        assert!(stored.is_none());
        let secret_identity =
            json!({"method":"client_secret","tenant_id":Uuid::nil(),"client_id":Uuid::nil()});
        admin(
            &client,
            &admin_url,
            &operator.raw_token,
            reqwest::Method::PATCH,
            &format!("providers/{provider_id}"),
            json!({"foundry":secret_identity}),
            400,
        )
        .await;
        let restored = admin(
            &client,
            &admin_url,
            &operator.raw_token,
            reqwest::Method::PATCH,
            &format!("providers/{provider_id}"),
            json!({"foundry":secret_identity,"credential":"mock-client-secret"}),
            200,
        )
        .await;
        assert_eq!(restored["credential_configured"], true);
    }
    // A rename queued behind an identity change must read the committed identity,
    // not restore the old client-secret snapshot with a now-cleared credential.
    let id = Uuid::parse_str(provider_id).unwrap();
    let mut transaction = store.pool().begin().await.unwrap();
    sqlx::query("UPDATE provider_configs SET foundry=$2, credential_secret=NULL WHERE id=$1")
        .bind(id)
        .bind(sqlx::types::Json(
            json!({"method":"workload_identity","tenant_id":Uuid::nil(),"client_id":Uuid::nil()}),
        ))
        .execute(&mut *transaction)
        .await
        .unwrap();
    let rename = store.patch_provider_config(
        id,
        serde_json::from_value(json!({"name":"Renamed Foundry"})).unwrap(),
    );
    tokio::pin!(rename);
    assert!(
        tokio::time::timeout(Duration::from_millis(100), &mut rename)
            .await
            .is_err()
    );
    transaction.commit().await.unwrap();
    let renamed = rename.await.unwrap().unwrap();
    assert_eq!(renamed.name, "Renamed Foundry");
    assert_eq!(
        renamed.foundry.unwrap().method,
        gateway_core::foundry::FoundryIdentityMethod::WorkloadIdentity
    );
    assert!(!renamed.credential_configured);
    // Invalid edits roll back; a subsequent valid patch can still acquire the row.
    admin(&client, &admin_url, &operator.raw_token, reqwest::Method::PATCH, &format!("providers/{provider_id}"), json!({"foundry":{"method":"client_secret","tenant_id":Uuid::nil(),"client_id":Uuid::nil()}}), 400).await;
    admin(&client, &admin_url, &operator.raw_token, reqwest::Method::PATCH, &format!("providers/{provider_id}"), json!({"foundry":{"method":"client_secret","tenant_id":Uuid::nil(),"client_id":Uuid::nil()},"credential":"mock-client-secret"}), 200).await;
    let check_path = format!("providers/{provider_id}/verify-connection");
    let check_url = format!("{admin_url}/admin-ui/admin/{check_path}");
    assert_eq!(client.post(&check_url).send().await.unwrap().status(), 401);
    assert_eq!(mock.tokens.load(Ordering::SeqCst), 0);
    sqlx::query("UPDATE operator_tokens SET scopes=ARRAY['usage:read'] WHERE token_prefix=$1")
        .bind(&operator.token_prefix)
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(
        client
            .post(&check_url)
            .bearer_auth(&operator.raw_token)
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    assert_eq!(mock.tokens.load(Ordering::SeqCst), 0);
    sqlx::query("UPDATE operator_tokens SET scopes=ARRAY['*'] WHERE token_prefix=$1")
        .bind(&operator.token_prefix)
        .execute(store.pool())
        .await
        .unwrap();
    for (status, expected, code) in [
        (0, "passed", "project_read_verified"),
        (403, "inconclusive", "project_read_forbidden"),
        (401, "failed", "project_unauthorized"),
        (404, "failed", "project_not_found"),
        (429, "inconclusive", "project_throttled"),
        (503, "inconclusive", "project_unavailable"),
        (302, "failed", "project_redirect"),
        (400, "failed", "project_rejected"),
        (200, "failed", "project_response_invalid"),
    ] {
        mock.read_status.store(status, Ordering::SeqCst);
        let before = mock.tokens.load(Ordering::SeqCst);
        let result = admin(
            &client,
            &admin_url,
            &operator.raw_token,
            reqwest::Method::POST,
            &check_path,
            json!({}),
            200,
        )
        .await;
        assert_eq!(result["identity"]["status"], "passed");
        assert_eq!(result["project"]["status"], expected);
        assert_eq!(result["project"]["code"], code);
        assert!(result["instance_id"]
            .as_str()
            .is_some_and(|s| !s.is_empty()));
        assert_eq!(
            mock.tokens.load(Ordering::SeqCst),
            before + 1,
            "every check acquires a fresh token"
        );
        for secret in [
            "mock-client-secret",
            "mock-azure-access-token",
            "private upstream details",
            "nextLink",
        ] {
            assert!(!result.to_string().contains(secret));
        }
        assert!(
            mock.calls.lock().unwrap().is_empty(),
            "verification must never execute an agent"
        );
    }
    mock.read_status.store(0, Ordering::SeqCst);
    admin(
        &client,
        &admin_url,
        &operator.raw_token,
        reqwest::Method::POST,
        &format!("providers/{provider_id}/disable"),
        json!({}),
        200,
    )
    .await;
    assert!(store
        .foundry_config(Uuid::parse_str(provider_id).unwrap())
        .await
        .unwrap()
        .is_none());
    let result = admin(
        &client,
        &admin_url,
        &operator.raw_token,
        reqwest::Method::POST,
        &check_path,
        json!({}),
        200,
    )
    .await;
    assert_eq!(result["project"]["status"], "passed");
    admin(
        &client,
        &admin_url,
        &operator.raw_token,
        reqwest::Method::POST,
        &format!("providers/{provider_id}/enable"),
        json!({}),
        200,
    )
    .await;
    admin(
        &client,
        &admin_url,
        &operator.raw_token,
        reqwest::Method::PATCH,
        &format!("providers/{provider_id}"),
        json!({"credential":"invalid-secret"}),
        200,
    )
    .await;
    let before = mock.reads.load(Ordering::SeqCst);
    let result = admin(
        &client,
        &admin_url,
        &operator.raw_token,
        reqwest::Method::POST,
        &check_path,
        json!({}),
        200,
    )
    .await;
    assert_eq!(result["identity"]["code"], "identity_rejected");
    assert_eq!(result["project"]["status"], "skipped");
    assert_eq!(mock.reads.load(Ordering::SeqCst), before);
    assert!(!result.to_string().contains("never leak"));
    admin(
        &client,
        &admin_url,
        &operator.raw_token,
        reqwest::Method::PATCH,
        &format!("providers/{provider_id}"),
        json!({"credential":"mock-client-secret"}),
        200,
    )
    .await;
    admin(
        &client,
        &admin_url,
        &operator.raw_token,
        reqwest::Method::POST,
        &format!("providers/{}/verify-connection", Uuid::new_v4()),
        json!({}),
        404,
    )
    .await;
    // Reset counters used by the existing forwarding/cache assertions below.
    mock.tokens.store(0, Ordering::SeqCst);

    let key = VirtualKeyMaterial::generate().unwrap();
    let record = store.create_admin_key(serde_json::from_value(json!({"owner_type":"individual","policy":{"allowed_routes":["/services/*"],"allowed_providers":["internal-service"],"allowed_models":[],"allowed_services":["research","foundry"],"allow_streaming":true,"allow_tools":true}})).unwrap(),&key).await.unwrap();
    let outsider = VirtualKeyMaterial::generate().unwrap();
    store.create_admin_key(serde_json::from_value(json!({"owner_type":"individual","policy":{"allowed_routes":["/services/*"],"allowed_providers":["internal-service"],"allowed_models":[]}})).unwrap(),&outsider).await.unwrap();
    for (name, binding) in [
        (
            "research",
            json!({"mode":"registered_agent","provider_id":provider_id,"agent_name":"bing-research","agent_version":"2"}),
        ),
        (
            "foundry",
            json!({"mode":"endpoint_passthrough","provider_id":provider_id}),
        ),
    ] {
        let service = admin(&client,&admin_url,&operator.raw_token,reqwest::Method::POST,"services",json!({"name":name,"allowed_methods":["POST"],"foundry":binding,"access":{"authentication_profiles":{"revision":0,"profiles":[{"id":"callers","name":"Allowed callers","enabled":true,"type":"relayna_key_only"}],"bindings":[{"key_id":record.id,"profile_id":"callers"}]}}}),200).await;
        assert_eq!(service["missing_runtime_fields"], json!([]));
    }
    assert!(store
        .provider_health_check_targets()
        .await
        .unwrap()
        .iter()
        .all(|target| target.credential.as_deref() != Some("mock-client-secret")));
    admin(
        &client,
        &admin_url,
        &operator.raw_token,
        reqwest::Method::DELETE,
        &format!("providers/{provider_id}"),
        json!({}),
        409,
    )
    .await;
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let proxy = RelaynaPingoraProxy::new(
        Arc::new(store.clone()),
        Arc::new(RedisControlState::new(&redis).unwrap()),
        PingoraLiteLlmConfig::from_base_url(&upstream, "unused")
            .unwrap()
            .with_auth_runtime(auth),
    );
    std::thread::spawn(move || {
        let mut server = pingora_core::server::Server::new(None).unwrap();
        server.bootstrap();
        let mut service = pingora_proxy::http_proxy_service(&server.configuration, proxy);
        service.add_tcp(&format!("127.0.0.1:{port}"));
        server.add_service(service);
        server.run_forever();
    });
    let started = Instant::now();
    loop {
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "mock-test proxy did not start"
        );
        if tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_ok()
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    eprintln!("proxy ready");
    let endpoint = format!("http://127.0.0.1:{port}/services/research/responses");
    for raw in ["invalid-key", &outsider.raw_key] {
        let response = client
            .post(&endpoint)
            .bearer_auth(raw)
            .json(&json!({"input":"hello"}))
            .send()
            .await
            .unwrap();
        assert!(response.status().is_client_error());
    }
    assert_eq!(mock.tokens.load(Ordering::SeqCst), 0);
    assert!(mock.calls.lock().unwrap().is_empty());
    eprintln!("first valid request");
    let response = client
        .post(&endpoint)
        .bearer_auth(&key.raw_key)
        .header("api-key", "client-api-key")
        .header("cookie", "client-cookie")
        .json(&json!({"input":"hello"}))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let text = response.text().await.unwrap();
    let value: Value = serde_json::from_str(&text).unwrap_or_else(|_| panic!("{status}: {text}"));
    assert_eq!(status, 200, "{value}");
    assert_eq!(
        value["output"][0]["content"][0]["annotations"][0]["url"],
        "https://example.com/source"
    );
    assert_eq!(
        mock.calls.lock().unwrap()[0].1["agent_reference"],
        json!({"type":"agent_reference","name":"bing-research","version":"2"})
    );
    let passthrough = format!("http://127.0.0.1:{port}/services/foundry/responses");
    let response = client
        .post(&passthrough)
        .bearer_auth(&key.raw_key)
        .json(&json!({"input":"hello","model":"deployment"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(
        mock.calls.lock().unwrap().last().unwrap().1["model"],
        "deployment"
    );
    assert_eq!(
        mock.tokens.load(Ordering::SeqCst),
        1,
        "token reused across services"
    );
    let start = Instant::now();
    let mut response = client
        .post(&endpoint)
        .bearer_auth(&key.raw_key)
        .json(&json!({"input":"hello","stream":true}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let first = response.chunk().await.unwrap().unwrap();
    assert!(start.elapsed() < Duration::from_millis(650), "SSE buffered");
    assert!(String::from_utf8_lossy(&first).contains("Grounded answer"));
    let rest = response.text().await.unwrap();
    assert!(rest.contains("url_citation"));
    for body in [
        json!({"input":"hello","model":"escape"}),
        json!({"input":"hello","agent_reference":{"name":"escape"}}),
        json!({"input":"hello","previous_response_id":"other-user"}),
        json!({"input":"hello","store":true}),
        json!({"input":"hello","background":true}),
    ] {
        let response = client
            .post(&endpoint)
            .bearer_auth(&key.raw_key)
            .json(&body)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let text = response.text().await.unwrap();
        assert_eq!(status, 400, "{text}");
        assert!(text.contains("invalid_foundry_request"));
    }
    for suffix in [
        "responses/other",
        "conversations",
        "responses?api-version=override",
    ] {
        let response = client
            .post(format!(
                "http://127.0.0.1:{port}/services/research/{suffix}"
            ))
            .bearer_auth(&key.raw_key)
            .json(&json!({"input":"hi"}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 400);
    }
    let response = client
        .post(&endpoint)
        .bearer_auth(&key.raw_key)
        .json(&json!({"input":"fail"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 429);
    assert_eq!(response.headers()["retry-after"], "2");
    // At the exact TPM boundary, one rewritten request consumes one reservation.
    // Use both modes; Redis inspection catches even a one-token header-stage charge.
    let mut counters = redis::Client::open(redis.clone())
        .unwrap()
        .get_multiplexed_async_connection()
        .await
        .unwrap();
    for target in [&endpoint, &passthrough] {
        let payload = json!({"input":"TPM boundary","model":"deployment","max_output_tokens":12});
        // Registered agents forbid model overrides.
        let payload = if target == &endpoint {
            json!({"input":"TPM boundary","max_output_tokens":12})
        } else {
            payload
        };
        let baseline = client
            .post(target)
            .bearer_auth(&key.raw_key)
            .json(&payload)
            .send()
            .await
            .unwrap();
        assert_eq!(baseline.status(), 200);
        let rewritten = mock.calls.lock().unwrap().last().unwrap().1.clone();
        let estimate = estimate_generation_tokens(&serde_json::to_vec(&rewritten).unwrap());
        store
            .patch_admin_key(
                record.id,
                AdminKeyPatch {
                    policy: Some(KeyPolicyPatch {
                        tpm_limit: Some(Some(i32::try_from(estimate).unwrap())),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let now = chrono::Utc::now();
        let windows: Vec<_> = [-1, 0, 1]
            .into_iter()
            .map(|m| {
                gateway_core::rate_limits::token_rate_limit_key(
                    record.id,
                    now + chrono::Duration::minutes(m),
                )
            })
            .collect();
        let _: i64 = redis::cmd("DEL")
            .arg(&windows)
            .query_async(&mut counters)
            .await
            .unwrap();
        let accepted = client
            .post(target)
            .bearer_auth(&key.raw_key)
            .json(&payload)
            .send()
            .await
            .unwrap();
        assert_eq!(accepted.status(), 200, "{}", accepted.text().await.unwrap());
        let counts: Vec<Option<i64>> = redis::cmd("MGET")
            .arg(&windows)
            .query_async(&mut counters)
            .await
            .unwrap();
        assert_eq!(counts.into_iter().flatten().sum::<i64>(), estimate);
        // Seed adjacent windows to keep the rejection assertion stable across a
        // minute rollover, without touching any other test key's counters.
        for window in &windows {
            let _: () = redis::cmd("SET")
                .arg(window)
                .arg(estimate)
                .arg("EX")
                .arg(70)
                .query_async(&mut counters)
                .await
                .unwrap();
        }
        let before = mock.calls.lock().unwrap().len();
        let denied = client
            .post(target)
            .bearer_auth(&key.raw_key)
            .json(&payload)
            .send()
            .await
            .unwrap();
        assert_eq!(denied.status(), 429);
        assert_eq!(mock.calls.lock().unwrap().len(), before);
        store
            .patch_admin_key(
                record.id,
                AdminKeyPatch {
                    policy: Some(KeyPolicyPatch {
                        tpm_limit: Some(None),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let _: i64 = redis::cmd("DEL")
            .arg(&windows)
            .query_async(&mut counters)
            .await
            .unwrap();
    }
    // Update policies without restarting the proxy; complete request fields must
    // be checked even though Pingora admits headers before reading the body.
    for (patch, payload) in [
        (
            json!({"allow_streaming":false}),
            json!({"input":"hello","stream":true}),
        ),
        (
            json!({"allowed_models":["permitted"]}),
            json!({"input":"hello","model":"forbidden"}),
        ),
        (
            json!({"allow_tools":false}),
            json!({"input":"hello","tools":[{"type":"web_search"}]}),
        ),
        (
            json!({"max_request_body_bytes":8}),
            json!({"input":"a body that is too long"}),
        ),
    ] {
        admin(
            &client,
            &admin_url,
            &operator.raw_token,
            reqwest::Method::PATCH,
            &format!("keys/{}", record.id),
            json!({"policy":patch}),
            200,
        )
        .await;
        let before = mock.calls.lock().unwrap().len();
        let response = client
            .post(&passthrough)
            .bearer_auth(&key.raw_key)
            .json(&payload)
            .send()
            .await
            .unwrap();
        assert!(
            response.status().is_client_error(),
            "{}",
            response.text().await.unwrap()
        );
        assert_eq!(
            mock.calls.lock().unwrap().len(),
            before,
            "denial must not execute an agent"
        );
        admin(&client,&admin_url,&operator.raw_token,reqwest::Method::PATCH,&format!("keys/{}",record.id),json!({"policy":{"allow_streaming":true,"allowed_models":[],"allow_tools":true,"max_request_body_bytes":1048576}}),200).await;
    }
    admin(
        &client,
        &admin_url,
        &operator.raw_token,
        reqwest::Method::PATCH,
        &format!("keys/{}", record.id),
        json!({"disabled":true}),
        200,
    )
    .await;
    assert_eq!(
        client
            .post(&endpoint)
            .bearer_auth(&key.raw_key)
            .json(&json!({"input":"disabled"}))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    admin(
        &client,
        &admin_url,
        &operator.raw_token,
        reqwest::Method::PATCH,
        &format!("keys/{}", record.id),
        json!({"disabled":false}),
        200,
    )
    .await;
    // Configuration edits preserve bindings and reject mixed generic/Foundry settings.
    admin(&client,&admin_url,&operator.raw_token,reqwest::Method::PATCH,"services/research",json!({"foundry":{"mode":"registered_agent","provider_id":provider_id,"agent_name":"updated-agent"}}),200).await;
    admin(
        &client,
        &admin_url,
        &operator.raw_token,
        reqwest::Method::PATCH,
        "services/research",
        json!({"upstream_base_url":"https://evil.example"}),
        400,
    )
    .await;
    admin(
        &client,
        &admin_url,
        &operator.raw_token,
        reqwest::Method::PATCH,
        &format!("providers/{provider_id}"),
        json!({"base_url":"https://evil.example/api/projects/demo"}),
        400,
    )
    .await;
    admin(
        &client,
        &admin_url,
        &operator.raw_token,
        reqwest::Method::PATCH,
        "services/research",
        json!({"studio_service_id":"should-not-be-importable"}),
        400,
    )
    .await;
    admin(&client, &admin_url, &operator.raw_token, reqwest::Method::POST,
        "services", json!({"name":"invalid-foundry", "allowed_methods":["POST"],
        "studio_service_id":"not-allowed", "foundry":{"mode":"endpoint_passthrough","provider_id":provider_id}}), 400).await;
    admin(
        &client,
        &admin_url,
        &operator.raw_token,
        reqwest::Method::POST,
        "services",
        json!({"name":"invalid-foundry", "allowed_methods":["GET"],
        "foundry":{"mode":"endpoint_passthrough","provider_id":provider_id}}),
        400,
    )
    .await;
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert_eq!(
        client
            .post(&endpoint)
            .bearer_auth(&key.raw_key)
            .json(&json!({"input":"renew"}))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    assert!(mock.tokens.load(Ordering::SeqCst) >= 2);
    admin(
        &client,
        &admin_url,
        &operator.raw_token,
        reqwest::Method::PATCH,
        &format!("providers/{provider_id}"),
        json!({"credential":"invalid-secret"}),
        200,
    )
    .await;
    let count = mock.calls.lock().unwrap().len();
    let response = client
        .post(&endpoint)
        .bearer_auth(&key.raw_key)
        .json(&json!({"input":"hi"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 502);
    let text = response.text().await.unwrap();
    assert!(text.contains("foundry_credential_unavailable"));
    assert!(!text.contains("never leak"));
    assert_eq!(mock.calls.lock().unwrap().len(), count);
    admin(
        &client,
        &admin_url,
        &operator.raw_token,
        reqwest::Method::POST,
        &format!("providers/{provider_id}/disable"),
        json!({}),
        200,
    )
    .await;
    let tokens = mock.tokens.load(Ordering::SeqCst);
    assert_eq!(
        client
            .post(&endpoint)
            .bearer_auth(&key.raw_key)
            .json(&json!({"input":"hi"}))
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    assert_eq!(mock.tokens.load(Ordering::SeqCst), tokens);
    // Omission retains the binding; explicit null removes it without recreating
    // the service or losing its caller authentication policy.
    let saved = store.get_service("research").await.unwrap().unwrap();
    let unchanged = admin(
        &client,
        &admin_url,
        &operator.raw_token,
        reqwest::Method::PATCH,
        "services/research",
        json!({"timeout_ms":119000}),
        200,
    )
    .await;
    assert_eq!(
        unchanged["foundry"],
        serde_json::to_value(&saved.foundry).unwrap()
    );
    let cleared = admin(
        &client,
        &admin_url,
        &operator.raw_token,
        reqwest::Method::PATCH,
        "services/research",
        json!({"foundry":null}),
        200,
    )
    .await;
    assert!(cleared["foundry"].is_null());
    assert_eq!(
        cleared["access"],
        serde_json::to_value(&saved.access).unwrap()
    );
    let provider_reference: Option<Uuid> = sqlx::query_scalar(
        "SELECT foundry_provider_id FROM service_registrations WHERE name='research'",
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert!(provider_reference.is_none());
    assert_eq!(
        client
            .post(&endpoint)
            .bearer_auth(&key.raw_key)
            .json(&json!({"input":"hi"}))
            .send()
            .await
            .unwrap()
            .status(),
        409
    );
    // A replacement binding remains supported and is still validated.
    admin(
        &client,
        &admin_url,
        &operator.raw_token,
        reqwest::Method::PATCH,
        "services/research",
        json!({"foundry":{"mode":"endpoint_passthrough","provider_id":Uuid::new_v4()}}),
        400,
    )
    .await;
    admin(
        &client,
        &admin_url,
        &operator.raw_token,
        reqwest::Method::PATCH,
        "services/research",
        json!({"foundry":saved.foundry}),
        200,
    )
    .await;
    let ordinary = serve(Router::new().route(
        "/responses",
        post(|headers: HeaderMap, Json(body): Json<Value>| async move {
            assert_eq!(
                headers["authorization"],
                "Bearer replacement-service-secret"
            );
            assert!(body.get("agent").is_none());
            Json(json!({"converted":true}))
        }),
    ))
    .await;
    let converted = admin(&client,&admin_url,&operator.raw_token,reqwest::Method::PATCH,"services/research",json!({"foundry":null,"upstream_base_url":ordinary,"credential":"replacement-service-secret"}),200).await;
    assert!(converted["foundry"].is_null());
    assert_eq!(converted["name"], "research");
    assert_eq!(
        converted["created_at"],
        serde_json::to_value(saved.created_at).unwrap()
    );
    assert_eq!(converted["access"], cleared["access"]);
    let calls_before = mock.calls.lock().unwrap().len();
    let response = client
        .post(&endpoint)
        .bearer_auth(&key.raw_key)
        .json(&json!({"input":"hi"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.json::<Value>().await.unwrap()["converted"], true);
    assert_eq!(mock.calls.lock().unwrap().len(), calls_before);
    let rows = store
        .traffic_history(gateway_core::traffic::TrafficQuery::default())
        .await
        .unwrap();
    assert!(!rows.is_empty());
    let accounted: i64 = sqlx::query_scalar("SELECT count(*) FROM usage_events WHERE input_tokens=2 AND output_tokens=3 AND total_tokens=5")
        .fetch_one(store.pool()).await.unwrap();
    assert!(
        accounted >= 4,
        "JSON and SSE usage must both be recorded: {accounted}"
    );
    let serialized = serde_json::to_string(&rows).unwrap();
    for secret in [
        &key.raw_key,
        "mock-client-secret",
        "mock-azure-access-token",
    ] {
        assert!(!serialized.contains(secret));
    }
    store.pool().close().await;
    sqlx::query(&format!("DROP DATABASE {name} WITH (FORCE)"))
        .execute(&parent)
        .await
        .unwrap();
    std::env::remove_var("GATEWAY_FOUNDRY_MOCK_TOKEN_ENDPOINT");
}
